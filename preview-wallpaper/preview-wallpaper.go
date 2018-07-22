package main

import (
	"fmt"
	"log"
	"os"
	"path/filepath"
	"time"

	lib "github.com/awused/windows-wallpapers/change-wallpaper-lib"
)

const errorLog = `C:\Logs\preview-wallpaper-error.log`

func main() {
	f, err := os.OpenFile(errorLog, os.O_RDWR|os.O_CREATE|os.O_APPEND, 0666)
	if err != nil {
		log.Fatalf("Error opening log file: %v", err)
	}
	defer f.Close()

	log.SetOutput(f)

	if len(os.Args) < 2 {
		log.Fatal("Missing input file")
	}

	w := os.Args[1]

	_, err = lib.Init()
	checkErr(err)
	defer lib.Cleanup()

	monitors, err := lib.GetMonitors()
	checkErr(err)

	outFiles := make([]string, len(monitors))
	scalingFactors := make([]int, len(monitors))
	scaledFiles := make([]string, len(monitors))

	tdir, err := lib.TempDir()
	checkErr(err)

MonitorLoop:
	for i, m := range monitors {
		outFiles[i] = filepath.Join(
			tdir, fmt.Sprintf("%dx%d", m.Width, m.Height)+"-preview.png")
		m.Wallpaper = outFiles[i]

		for _, s := range outFiles[:i] {
			if outFiles[i] == s {
				continue MonitorLoop
			}
		}

		scalingFactors[i], err = lib.GetScalingFactor(w, m.Width, m.Height, false)
		checkErr(err)

		for j, s := range scalingFactors[:i] {
			if scalingFactors[i] == s {
				err = lib.ProcessImage(scaledFiles[j], outFiles[i], m.Width, m.Height, false, false, true)
				checkErr(err)

				continue MonitorLoop
			}
		}

		scaledFiles[i], err = lib.GetScaledIntermediateFile(outFiles[i], scalingFactors[i])
		checkErr(err)

		err = lib.ProcessImage(w, outFiles[i], m.Width, m.Height, false, true, true)
		checkErr(err)
	}

	err = lib.SetMonitorWallpapers(monitors)
	checkErr(err)

	// Windows will fail to read the wallpapers if we delete them too fast
	<-time.After(5 * time.Second)
}

func checkErr(err error) {
	if err != nil {
		log.Fatal(err)
	}
}
