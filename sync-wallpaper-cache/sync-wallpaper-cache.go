package main

import (
	"log"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"

	"github.com/awused/go-strpick/persistent"
	lib "github.com/awused/windows-wallpapers/change-wallpaper-lib"
)

const errorLog = `C:\Logs\sync-wallpaper-cache-error.log`

// Deletes all png files that don't correspond to an existing original wallpaper
// Does not remove cached wallpapers for monitors that don't exist, users will have to remove those manually
func main() {
	f, err := os.OpenFile(errorLog, os.O_RDWR|os.O_CREATE|os.O_APPEND, 0666)
	if err != nil {
		log.Fatalf("Error opening file: %v", err)
	}
	defer f.Close()

	log.SetOutput(f)

	c, err := lib.Init()
	if err != nil {
		log.Fatal(err)
	}
	defer lib.Cleanup()

	picker, err := persistent.NewPicker(c.UsedWallpapersDBDir)
	if err != nil {
		log.Fatal(err)
	}
	defer picker.Close()

	monitors, err := lib.GetMonitors()
	if err != nil {
		log.Fatal(err)
	}

	originals, err := lib.GetAllOriginals()
	if err != nil {
		log.Fatal(err)
	}

	err = picker.AddAll(originals)
	if err != nil {
		log.Fatal(err)
	}

	var count int32
	originalsProcessed := 0
	allValidFiles := &sync.Map{}
	var wg sync.WaitGroup

	for _, relPath := range originals {
		wg.Add(1)
		originalsProcessed++

		go func(relPath lib.RelativePath) {
			processed := cacheImageForMonitors(relPath, monitors, allValidFiles)
			atomic.AddInt32(&count, processed)
			wg.Done()
		}(relPath)

		// Run in batches so that we can clean up if necessary
		if originalsProcessed%200 == 0 {
			wg.Wait()

			// Intermediate files are stored as bitmaps, and can take a lot of space
			// 100 4K bitmaps at 8bpc is over 2GB, and many intermediate files will
			// exceed the resolution of the monitor
			if count > 100 {
				err = lib.PartialCleanup()
				count = 0
				if err != nil {
					log.Fatal(err.Error())
				}
			}
		}
	}

	wg.Wait()

	err = pruneCache(c, allValidFiles)
	if err != nil {
		log.Fatal(err)
	}

	err = picker.CleanDB()
	if err != nil {
		log.Fatal(err)
	}
}

func pruneCache(c *lib.Config, allValidFiles *sync.Map) error {
	return filepath.Walk(c.CacheDirectory, func(path string, f os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		if f.Mode().IsRegular() {
			absPath, err := filepath.Abs(path)
			_, valid := allValidFiles.Load(absPath)
			if err == nil && filepath.Ext(path) == ".png" && !valid {
				return os.Remove(path)
			}
		}

		return nil
	})
}

func cacheImageForMonitors(
	relPath lib.RelativePath,
	monitors []*lib.Monitor,
	allValidFiles *sync.Map) int32 {

	scaledFiles := make([]string, len(monitors))
	absPath, err := lib.GetFullInputPath(relPath)
	if err != nil {
		log.Fatal(err)
	}

	var count int32

	for i, m := range monitors {
		cropOffset := lib.GetConfigCropOffset(relPath, m)

		outFile, err := lib.GetCacheImagePath(relPath, m, cropOffset)
		if err != nil {
			log.Fatal(err)
		}

		allValidFiles.Store(outFile, true)

		// Possible for an earlier monitor to have already created the right file
		doScale, err := lib.ShouldProcessImage(absPath, outFile)
		if err != nil {
			log.Fatal(err.Error())
		}

		if !doScale {
			continue
		}

		count++

		wipFile := outFile + "-wip.png"

		po := lib.ProcessOptions{
			Input:      absPath,
			Output:     wipFile,
			Width:      m.Width,
			Height:     m.Height,
			Denoise:    true,
			Flatten:    true,
			CropOrPad:  true,
			CropOffset: cropOffset}

		scaledFiles[i], err = lib.GetScaledIntermediateFile(po)
		if err != nil {
			log.Fatal(err)
		}

		for _, sf := range scaledFiles[:i] {
			if scaledFiles[i] == sf {
				po.Input = sf
				// Scaled files have already been denoised and cropped/padded
				po.Denoise = false
				po.CropOrPad = false
				break
			}
		}

		err = lib.ProcessImage(po)
		if err != nil {
			log.Fatal(err)
		}

		// Renaming should be atomic enough for our purposes
		err = os.Rename(wipFile, outFile)
		if err != nil {
			log.Fatal(err)
		}
	}
	return count
}
