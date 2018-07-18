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

	c, err := lib.ReadConfig()
	if err != nil {
		log.Fatal(err)
	}
	defer lib.Cleanup()

	monitors, err := lib.GetMonitors()
	if err != nil {
		log.Fatal(err)
	}

	err = lib.SetupCacheDirectories(monitors)
	if err != nil {
		log.Fatal(err)
	}

	inputDirectories, err := lib.WalkAllInputDirectories()
	if err != nil {
		log.Fatal(err)
	}

	count := 0
	allValidFiles := make(map[string]bool)

	for _, inp := range inputDirectories {
		for _, relPath := range inp.Files {
			scalingFactors := make([]int, len(monitors))
			scaledFiles := make([]string, len(monitors))
			absPath, err := lib.GetFullInputPath(inp, relPath)
			if err != nil {
				log.Fatal(err)
			}

			for i, m := range monitors {
				outFile, err := lib.GetCacheImagePath(inp, relPath, m)
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
				if count%100 == 0 {
					err = lib.Cleanup()
					if err != nil {
						log.Fatal(err.Error())
					}
				}

				scalingFactors[i], err = lib.GetScalingFactor(absPath, m.Width, m.Height, false)
				if err != nil {
					log.Fatal(err)
				}

				wipFile := outFile + "-wip.png"

				match := false
				for j, s := range scalingFactors[:i] {
					if scalingFactors[i] == s {
						match = true
						err = lib.ProcessImage(scaledFiles[j], wipFile, m.Width, m.Height, false, false)
						if err != nil {
							log.Fatal(err)
						}

						break
					}
				}

				if !match {
					scaledFiles[i], err = lib.GetScaledIntermediateFile(wipFile, scalingFactors[i])
					if err != nil {
						log.Fatal(err)
					}

					lib.ProcessImage(absPath, wipFile, m.Width, m.Height, false, true)
				}

				err = os.Rename(wipFile, outFile)
				if err != nil {
					log.Fatal(err)
				}
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
