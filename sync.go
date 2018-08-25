package main

import (
	"math"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"

	"github.com/awused/go-strpick/persistent"
	lib "github.com/awused/wallpapers/lib"
	"github.com/urfave/cli"
)

const limit = "limit"

func syncCommand() cli.Command {
	cmd := cli.Command{}
	cmd.Name = "sync"
	cmd.Usage = "Prepopulate the cache of scaled files and remove stale files"
	cmd.Description = "Does not remove cached wallpapers for disconnected " +
		"monitors, remove those manually"
	cmd.Flags = []cli.Flag{
		cli.Int64Flag{
			Name:  limit + ", l",
			Value: math.MaxInt64,
			Usage: "The maximum number of original wallpapers to scale.",
		},
	}

	cmd.Action = syncAction

	return cmd
}

// Deletes all png files that don't correspond to an existing original wallpaper
// Does not remove cached wallpapers for monitors that don't exist, users will have to remove those manually
func syncAction(c *cli.Context) error {
	var syncLimit int64 = c.Int64(limit)

	conf, err := lib.GetConfig()
	checkErr(err)

	picker, err := persistent.NewPicker(conf.DatabaseDir)
	checkErr(err)
	defer picker.Close()

	monitors, err := lib.GetMonitors()
	checkErr(err)

	originals, err := lib.GetAllOriginals()
	checkErr(err)

	err = picker.AddAll(originals)
	checkErr(err)

	var count int32

	originalsProcessed := 0
	allValidFiles := &sync.Map{}
	var wg sync.WaitGroup

	for _, relPath := range originals {
		wg.Add(1)
		originalsProcessed++

		go func(relPath lib.RelativePath) {
			processed := cacheImageForMonitors(
				relPath, monitors, allValidFiles, &syncLimit)
			atomic.AddInt32(&count, processed)
			wg.Done()
		}(relPath)

		// Run in batches so that we can clean up if necessary
		if originalsProcessed%200 == 0 {
			wg.Wait()

			if syncLimit < 0 {
				// Do not prune the cache, do not clean the DB
				return nil
			}

			// Intermediate files are stored as bitmaps, and can take a lot of space
			// 100 4K bitmaps at 8bpc is over 2GB, and many intermediate files will
			// exceed the resolution of the monitor
			if count > 100 {
				err = lib.PartialCleanup()
				count = 0
				checkErr(err)
			}
		}
	}

	wg.Wait()

	err = pruneCache(conf.CacheDirectory, allValidFiles)
	checkErr(err)

	err = picker.CleanDB()
	checkErr(err)
	return nil
}

func pruneCache(cacheDir string, allValidFiles *sync.Map) error {
	return filepath.Walk(cacheDir, func(path string, f os.FileInfo, err error) error {
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
	allValidFiles *sync.Map,
	syncLimit *int64) int32 {
	if atomic.LoadInt64(syncLimit) < 0 {
		return 0
	}

	scaledFiles := make([]string, len(monitors))
	absPath, err := lib.GetFullInputPath(relPath)
	checkErr(err)

	var count int32

	for i, m := range monitors {
		imageProps := lib.GetConfigImageProps(relPath, m)

		outFile, err := lib.GetCacheImagePath(relPath, m, imageProps)
		checkErr(err)

		allValidFiles.Store(outFile, true)

		// Possible for an earlier monitor to have already created the right file
		doScale, err := lib.ShouldProcessImage(absPath, outFile)
		checkErr(err)

		if !doScale {
			continue
		}

		if count == 0 {
			if atomic.AddInt64(syncLimit, -1) < 0 {
				break
			}
		}

		count++

		// TODO -- Remove the chance for filename collisions here
		wipFile := outFile + "-wip.png"

		po := lib.ProcessOptions{
			Input:      absPath,
			Output:     wipFile,
			Width:      m.Width,
			Height:     m.Height,
			Denoise:    true,
			Flatten:    true,
			CropOrPad:  true,
			ImageProps: imageProps}

		scaledFiles[i], err = lib.GetScaledIntermediateFile(po)
		checkErr(err)

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
		checkErr(err)

		// Renaming should be atomic enough for our purposes
		err = os.Rename(wipFile, outFile)
		checkErr(err)
	}
	return count
}
