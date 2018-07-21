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

	c, err := lib.ReadConfig()
	checkErr(err)
	defer lib.Cleanup()

	monitors, err := lib.GetMonitors()
	checkErr(err)

	err = lib.SetupCacheDirectories(monitors)
	checkErr(err)

	inputDirectories, err := lib.WalkAllInputDirectories()
	checkErr(err)

	picker, err := persistent.NewPicker(c.UsedWallpapersDBDir)
	checkErr(err)
	defer picker.Close()

	// Go from the unique ID (hash) back to the directory and file
	fileLookups := make(map[string][]int)

	for i, inp := range inputDirectories {
		for j, relPath := range inp.Files {
			uniqueId := lib.MakeUniqueIdForFile(inp, relPath)
			err = picker.Add(uniqueId)
			checkErr(err)

			if lookup, ok := fileLookups[uniqueId]; ok {
				// Hash collision, this is incredibly likely to be due to two input
				// directories containing files with identical relative paths
				s, _ := lib.GetFullInputPath(inp, relPath)
				otherFile, _ := lib.GetFullInputPath(inputDirectories[lookup[0]], inputDirectories[lookup[0]].Files[lookup[1]])
				log.Fatalf("Hash collision between [%s] and [%s], change your hash prefixes", s, otherFile)
			}

			fileLookups[uniqueId] = []int{i, j}
		}
	}

	sz, err := picker.Size()
	checkErr(err)
	if sz == 0 {
		log.Fatal("No wallpapers present in any OriginalsDirectory")
	}

	var fileIDs []string
	if len(monitors) <= sz {
		fileIDs, err = picker.UniqueN(len(monitors))
	} else {
		fileIDs, err = picker.NextN(len(monitors))
	}
	checkErr(err)

	//scaledFiles := make([]string, len(monitors))
	for i, inputId := range fileIDs {
		m := monitors[i]
		inputDirectory := inputDirectories[fileLookups[inputId][0]]
		inputRelPath := inputDirectory.Files[fileLookups[inputId][1]]
		inputAbsPath, err := lib.GetFullInputPath(inputDirectory, inputRelPath)
		checkErr(err)

		scaledFile, err := lib.GetCacheImagePath(inputDirectory, inputRelPath, m)
		checkErr(err)

		doScale, err := lib.ShouldProcessImage(inputAbsPath, scaledFile)
		checkErr(err)

		if doScale {
			err = lib.ProcessImage(inputAbsPath, scaledFile, m.Width, m.Height, false, true, true)
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
