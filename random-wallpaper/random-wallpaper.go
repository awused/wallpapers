package main

import (
	"log"
	"os"

	"github.com/awused/go-strpick/persistent"
	lib "github.com/awused/windows-wallpapers/change-wallpaper-lib"
)

const errorLog = `C:\Logs\random-wallpaper-error.log`

func main() {
	f, err := os.OpenFile(errorLog, os.O_RDWR|os.O_CREATE|os.O_APPEND, 0666)
	if err != nil {
		log.Fatalf("Error opening file: %v", err)
	}
	defer f.Close()

	log.SetOutput(f)

	c, err := lib.Init()
	checkErr(err)
	defer lib.Cleanup()

	picker, err := persistent.NewPicker(c.UsedWallpapersDBDir)
	checkErr(err)
	defer picker.Close()

	// TODO -- move this behaviour to a --cron or --unlocked flag
	locked, err := lib.CheckIfLocked()
	checkErr(err)
	if locked {
		// Silently exit, this isn't an error
		return
	}

	monitors, err := lib.GetMonitors()
	checkErr(err)

	originals, err := lib.GetAllOriginals()
	checkErr(err)

	err = picker.AddAll(originals)
	checkErr(err)

	sz, err := picker.Size()
	checkErr(err)
	if sz == 0 {
		log.Fatal("No wallpapers present in OriginalDirectory")
	}

	inputRelPaths, err := picker.TryUniqueN(len(monitors))
	checkErr(err)

	//scaledFiles := make([]string, len(monitors))
	for i, relPath := range inputRelPaths {
		m := monitors[i]
		cropOffset := lib.GetConfigCropOffset(relPath, m)

		absPath, err := lib.GetFullInputPath(relPath)
		checkErr(err)

		cachedFile, err := lib.GetCacheImagePath(relPath, m, cropOffset)
		checkErr(err)

		doScale, err := lib.ShouldProcessImage(absPath, cachedFile)
		checkErr(err)

		if doScale {
			po := lib.ProcessOptions{
				Input:      absPath,
				Output:     cachedFile,
				Width:      m.Width,
				Height:     m.Height,
				Denoise:    true,
				Flatten:    true,
				CropOrPad:  true,
				CropOffset: cropOffset}
			err = lib.ProcessImage(po)
			checkErr(err)
		}

		m.Wallpaper = cachedFile
	}

	err = lib.SetMonitorWallpapers(monitors)
	checkErr(err)

	//err = lib.CombineImages(scaledFiles, monitors, c.WallpaperFile)
	//checkErr(err)

	//err = lib.ChangeBackground()
	//checkErr(err)
}

func checkErr(err error) {
	if err != nil {
		log.Fatal(err)
	}
}
