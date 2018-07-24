package main

import (
	"fmt"
	"log"
	"os"
	"path/filepath"
	"time"

	lib "github.com/awused/windows-wallpapers/change-wallpaper-lib"
	"github.com/urfave/cli"
)

const errorLog = `C:\Logs\preview-wallpaper-error.log`
const horizontal = "horizontal"
const vertical = "vertical"

func main() {
	app := cli.NewApp()
	app.Usage = "Preview wallpaper on all monitors"
	app.Flags = []cli.Flag{
		cli.Float64Flag{
			Name:  vertical + ", y",
			Value: 0,
			Usage: "Vertical offset, as a percentage of the file's height." +
				"Positive values move the viewport upwards",
		},
		cli.Float64Flag{
			Name:  horizontal + ", x",
			Value: 0,
			Usage: "Horizontal offset, as a percentage of the file's width." +
				"Positive values move the viewport right",
		},
	}

	app.Action = preview

	err := app.Run(os.Args)
	checkErr(err)
}

func preview(c *cli.Context) error {
	f, err := os.OpenFile(errorLog, os.O_RDWR|os.O_CREATE|os.O_APPEND, 0666)
	if err != nil {
		log.Fatalf("Error opening log file: %v", err)
	}
	defer f.Close()

	log.SetOutput(f)

	if c.NArg() == 0 {
		log.Fatal("Missing input file")
	}

	w, err := filepath.Abs(c.Args().First())
	checkErr(err)

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
			tdir, fmt.Sprintf("%dx%d", m.Width, m.Height)+"-preview.bmp")
		m.Wallpaper = outFiles[i]

		for _, s := range outFiles[:i] {
			if outFiles[i] == s {
				continue MonitorLoop
			}
		}

		po := lib.ProcessOptions{
			Input:   w,
			Output:  outFiles[i],
			Width:   m.Width,
			Height:  m.Height,
			Denoise: true,
			Flatten: true,
			Offset: lib.CropOffset{
				Vertical:   c.Float64(vertical),
				Horizontal: c.Float64(horizontal)}}

		scalingFactors[i], err = lib.GetScalingFactor(w, m.Width, m.Height, false)
		checkErr(err)

		for j, s := range scalingFactors[:i] {
			if scalingFactors[i] == s {
				po.Input = scaledFiles[j]
				// Scaled files have already been denoised
				po.Denoise = false
				err = lib.ProcessImage(po)
				checkErr(err)

				continue MonitorLoop
			}
		}

		scaledFiles[i], err = lib.GetScaledIntermediateFile(outFiles[i], scalingFactors[i])
		checkErr(err)

		err = lib.ProcessImage(po)
		checkErr(err)
	}

	err = lib.SetMonitorWallpapers(monitors)
	checkErr(err)

	// Windows will fail to read the wallpapers if we delete them too fast
	<-time.After(5 * time.Second)
	return nil
}

func checkErr(err error) {
	if err != nil {
		log.Fatal(err)
	}
}
