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

	monitors, err := lib.GetMonitors()
	checkErr(err)

	picker, err := persistent.NewPicker(c.UsedWallpapersDBDir)
	checkErr(err)
	defer picker.Close()

	originals, err := lib.GetAllOriginals()
	checkErr(err)

	err = picker.AddAll(originals)
	checkErr(err)

	sz, err := picker.Size()
	checkErr(err)
	if sz == 0 {
		log.Fatal("No wallpapers present in OriginalDirectory")
	}

	var inputRelPaths []string
	if len(monitors) <= sz {
		inputRelPaths, err = picker.UniqueN(len(monitors))
	} else {
		inputRelPaths, err = picker.NextN(len(monitors))
	}
	checkErr(err)

	//scaledFiles := make([]string, len(monitors))
	for i, relPath := range inputRelPaths {
		m := monitors[i]
		cropOffset := lib.GetConfigCropOffset(relPath, m)

		absPath, err := lib.GetFullInputPath(relPath)
		checkErr(err)

		scaledFile, err := lib.GetCacheImagePath(relPath, m, cropOffset)
		checkErr(err)

		doScale, err := lib.ShouldProcessImage(absPath, scaledFile)
		checkErr(err)

		if doScale {
			po := lib.ProcessOptions{
				Input:   absPath,
				Output:  scaledFile,
				Width:   m.Width,
				Height:  m.Height,
				Denoise: true,
				Flatten: true,
				Offset:  cropOffset}
			err = lib.ProcessImage(po)
			checkErr(err)
		}

		m.Wallpaper = scaledFile
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
