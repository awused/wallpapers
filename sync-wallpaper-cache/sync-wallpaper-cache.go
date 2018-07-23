package main

import (
	"log"
	"os"
	"path/filepath"
	"strings"

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

	monitors, err := lib.GetMonitors()
	if err != nil {
		log.Fatal(err)
	}

	originals, err := lib.GetAllOriginals()
	if err != nil {
		log.Fatal(err)
	}

	count := 0
	allValidFiles := make(map[string]bool)

	for _, relPath := range originals {
		scalingFactors := make([]int, len(monitors))
		scaledFiles := make([]string, len(monitors))
		absPath, err := lib.GetFullInputPath(relPath)
		if err != nil {
			log.Fatal(err)
		}

		for i, m := range monitors {
			outFile, err := lib.GetCacheImagePath(relPath, m)
			if err != nil {
				log.Fatal(err)
			}

			allValidFiles[filepath.Base(outFile)] = true

			// Possible for an earlier monitor to have already created the right file
			doScale, err := lib.ShouldProcessImage(absPath, outFile)
			if err != nil {
				log.Fatal(err.Error())
			}

			if !doScale {
				continue
			}

			count++
			if count%500 == 0 {
				err = lib.PartialCleanup()
				if err != nil {
					log.Fatal(err.Error())
				}
			}

			scalingFactors[i], err = lib.GetScalingFactor(absPath, m.Width, m.Height, false)
			if err != nil {
				log.Fatal(err)
			}

			wipFile := outFile + "-wip.png"

			po := lib.ProcessOptions{
				Input:   absPath,
				Output:  wipFile,
				Width:   m.Width,
				Height:  m.Height,
				Denoise: true,
				Flatten: true}

			match := false
			for j, s := range scalingFactors[:i] {
				if scalingFactors[i] == s {
					match = true
					po.Input = scaledFiles[j]
					// Scaled files have already been denoised
					po.Denoise = false

					break
				}
			}

			if !match {
				scaledFiles[i], err = lib.GetScaledIntermediateFile(wipFile, scalingFactors[i])
				if err != nil {
					log.Fatal(err)
				}
			}

			err = lib.ProcessImage(po)
			if err != nil {
				log.Fatal(err)
			}

			err = os.Rename(wipFile, outFile)
			if err != nil {
				log.Fatal(err)
			}
		}
	}

	err = pruneCache(c, allValidFiles)
	if err != nil {
		log.Fatal(err)
	}
}

func pruneCache(c *lib.Config, valid map[string]bool) error {
	return filepath.Walk(c.CacheDirectory, func(path string, f os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		if f.Mode().IsRegular() {
			if strings.HasSuffix(path, "png") && !valid[filepath.Base(path)] {
				return os.Remove(path)
			}
		}

		return nil
	})
}
