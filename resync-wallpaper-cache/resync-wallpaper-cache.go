package main

import (
	"log"
	"os"

	lib "github.com/awused/windows-wallpapers/change-wallpaper-lib"
)

//const errorLog = `C:\Logs\resync-wallpaper-cache-error.log`

func main() {
	//f, err := os.OpenFile(errorLog, os.O_RDWR | os.O_CREATE | os.O_APPEND, 0666)
	//if err != nil {
	//   log.Fatalf("Error opening file: %v", err)
	//}
	//defer f.Close()

	//log.SetOutput(f)

	_, err := lib.ReadConfig()
	if err != nil {
		log.Fatal(err)
	}

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

	for _, inp := range inputDirectories {
		for _, relPath := range inp.Files {
			for _, m := range monitors {
				inputAbsPath, err := lib.GetFullInputPath(inp, relPath)
				if err != nil {
					log.Fatal(err)
				}
				oldOutFile, err := lib.GetOldCacheImagePath(inp, relPath, m)
				if err != nil {
					log.Fatal(err)
				}
				newOutFile, err := lib.GetCacheImagePath(inp, relPath, m)
				if err != nil {
					log.Fatal(err)
				}

				// If the old file is invalid, don't rename
				doScale, err := lib.ShouldProcessImage(inputAbsPath, oldOutFile)
				if err != nil {
					log.Fatal(err.Error())
				}

				if doScale {
					continue
				}

				// New file is already valid, don't overwrite
				doScale, err = lib.ShouldProcessImage(inputAbsPath, newOutFile)
				if err != nil {
					log.Fatal(err.Error())
				}

				if !doScale {
					continue
				}

				log.Printf("Renaming [%s] to [%s]", oldOutFile, newOutFile)

				err = os.Rename(oldOutFile, newOutFile)
				if err != nil {
					log.Fatal(err)
				}
			}
		}
	}
}
