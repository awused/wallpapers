package main

import (
	"encoding/json"
	"io/ioutil"
	"log"
	"math/rand"
	"os"
	"time"

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

	allFilesUniqueIds := make(map[string]bool)
	// Lookup a file
	fileLookups := make(map[string][]int)

	for i, inp := range inputDirectories {
		for j, relPath := range inp.Files {
			uniqueId := lib.MakeUniqueIdForFile(inp, relPath)
			allFilesUniqueIds[uniqueId] = true

			if _, ok := fileLookups[uniqueId]; ok {
				s, _ := lib.GetFullInputPath(inp, relPath)
				log.Fatalf("Hash collision with file [%s], change your hash prefixes", s)
			}

			fileLookups[uniqueId] = []int{i, j}
		}
	}

	if len(allFilesUniqueIds) == 0 {
		log.Fatal("No wallpapers present in any OriginalsDirectory")
	}

	used := make(map[string]bool)
	using := make(map[string]bool)

	// Read previously used wallpapers so wallpapers can be randomly selected without replacement
	usedFile, err := os.Open(c.UsedWallpapersFile)
	if err == nil {
		decoder := json.NewDecoder(usedFile)
		err = decoder.Decode(&used)
		if err != nil {
			log.Fatal(err)
		}
	} else if os.IsNotExist(err) {
	} else {
		log.Fatal(err)
	}
	usedFile.Close()

	files := make([]string, 0, len(allFilesUniqueIds))
	for f, _ := range allFilesUniqueIds {
		if !used[f] {
			files = append(files, f)
		}
	}

	scaledFiles := make([]string, len(monitors))
	rand.Seed(time.Now().UnixNano())

	for i, m := range monitors {
		// If there are no wallpapers left use wallpapers from previous cycles
		if len(files) == 0 {
			for f, _ := range allFilesUniqueIds {
				if !using[f] {
					files = append(files, f)
				}
			}
			if len(used) != 0 {
				used = make(map[string]bool)
			}

			// If there are still no wallpapers left reuse wallpapers from this cycle
			if len(files) == 0 {
				for f, _ := range allFilesUniqueIds {
					files = append(files, f)
				}
			}
		}

		idx := rand.Intn(len(files))

		inputId := files[idx]
		inputDirectory := inputDirectories[fileLookups[inputId][0]]
		inputRelPath := inputDirectory.Files[fileLookups[inputId][1]]
		inputAbsPath, err := lib.GetFullInputPath(inputDirectory, inputRelPath)
		if err != nil {
			log.Fatal(err)
		}

		using[inputId] = true
		files[idx] = files[len(files)-1]
		files = files[:len(files)-1]

		scaledFiles[i], err = lib.GetCacheImagePath(inputDirectory, inputRelPath, m)
		if err != nil {
			log.Fatal(err)
		}

		doScale, err := lib.ShouldProcessImage(inputAbsPath, scaledFiles[i])
		if err != nil {
			log.Fatal(err.Error())
		}

		if doScale {
			err = lib.ProcessImage(inputAbsPath, scaledFiles[i], m.Width, m.Height, false, true)
			if err != nil {
				log.Fatal(err.Error())
			}
		}
	}

	err = lib.CombineImages(scaledFiles, monitors, c.WallpaperFile)
	if err != nil {
		log.Fatal(err)
	}

	err = lib.ChangeBackground()
	if err != nil {
		log.Fatal(err)
	}

	for f, _ := range using {
		used[f] = true
	}

	usedBytes, err := json.Marshal(used)
	if err != nil {
		log.Fatal(err)
	}

	err = ioutil.WriteFile(c.UsedWallpapersFile, usedBytes, 0)
	if err != nil {
		log.Fatal(err)
	}
}
