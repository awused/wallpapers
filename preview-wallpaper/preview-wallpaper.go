package main

import (
	"log"
	"os"
	"path/filepath"
	"strconv"
	"time"

	lib "github.com/awused/windows-wallpapers/change-wallpaper-lib"
)

const errorLog = `C:\Logs\preview-wallpaper-error.log`

func main() {
	f, err := os.OpenFile(errorLog, os.O_RDWR|os.O_CREATE|os.O_APPEND, 0666)
	if err != nil {
		log.Fatalf("Error opening file: %v", err)
	}
	defer f.Close()

	log.SetOutput(f)

	if len(os.Args) < 2 {
		log.Fatal("Missing input file")
	}

	w := os.Args[1]

	c, err := lib.ReadConfig()
	if err != nil {
		log.Fatal(err)
	}
	defer lib.Cleanup()

	monitors, err := lib.GetMonitors()
	if err != nil {
		log.Fatal(err)
	}

	outFiles := make([]string, len(monitors))
	scalingFactors := make([]int, len(monitors))
	scaledFiles := make([]string, len(monitors))

MonitorLoop:
	for i, m := range monitors {
		outFiles[i] = filepath.Join(c.TempDirectory, strconv.Itoa(i)+"-preview.png")
		scalingFactors[i], err = lib.GetScalingFactor(w, m.Width, m.Height, false)
		if err != nil {
			log.Fatal(err)
		}

		for j, s := range scalingFactors[:i] {
			if scalingFactors[i] == s {
				err = lib.ProcessImage(scaledFiles[j], outFiles[i], m.Width, m.Height, false, false)
				if err != nil {
					log.Fatal(err)
				}

				continue MonitorLoop
			}
		}

		scaledFiles[i], err = lib.GetScaledIntermediateFile(outFiles[i], scalingFactors[i])
		if err != nil {
			log.Fatal(err)
		}

		err = lib.ProcessImage(w, outFiles[i], m.Width, m.Height, false, true)
		if err != nil {
			log.Fatal(err)
		}

	}

	for i, m := range monitors {
		err = lib.SetMonitorWallpaper(m, outFiles[i])
		if err != nil {
			log.Fatal(err)
		}
	}

	// Windows will fail to read the wallpapers if we delete them too fast
	<-time.After(5 * time.Second)
}
