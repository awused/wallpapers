package main

import (
	"errors"
	"log"
	"path/filepath"
	"runtime"
	"time"

	lib "github.com/awused/wallpapers/lib"
	"github.com/urfave/cli/v2"
)

const horizontal = "horizontal"
const vertical = "vertical"
const top = "top"
const bottom = "bottom"
const left = "left"
const right = "right"
const background = "background"

func previewCommand() *cli.Command {
	cmd := &cli.Command{}
	cmd.Name = "preview"
	cmd.Usage = "Preview a single wallpaper on every monitor"
	cmd.ArgsUsage = "FILE"
	cmd.Before = beforeFunc
	cmd.Flags = []cli.Flag{
		&cli.Float64Flag{
			Name:    vertical,
			Aliases: []string{"v"},
			Value:   0,
			Usage: "Vertical offset, as a percentage of the file's height." +
				"Positive values move the viewport upwards",
		},
		&cli.Float64Flag{
			Name:    horizontal,
			Aliases: []string{"x"},
			Value:   0,
			Usage: "Horizontal offset, as a percentage of the file's width." +
				"Positive values move the viewport right",
		},
		&cli.IntFlag{
			Name:    top,
			Aliases: []string{"t"},
			Value:   0,
			Usage:   "Pixels to crop off the top, negative values pad",
		},
		&cli.IntFlag{
			Name:    bottom,
			Aliases: []string{"b"},
			Value:   0,
			Usage:   "Pixels to crop off the bottom, negative values pad",
		},
		&cli.IntFlag{
			Name:    left,
			Aliases: []string{"l"},
			Value:   0,
			Usage:   "Pixels to crop off the left side, negative values pad",
		},
		&cli.IntFlag{
			Name:    right,
			Aliases: []string{"r"},
			Value:   0,
			Usage:   "Pixels to crop off the right side, negative values pad",
		},
		&cli.StringFlag{
			Name:    background,
			Aliases: []string{"bg"},
			Value:   "black",
			Usage:   "Background colour to use when padding",
		},
	}

	cmd.Action = previewAction

	return cmd
}

func previewAction(c *cli.Context) error {
	if c.NArg() == 0 {
		checkErr(errors.New("Missing input file"))
	}

	w, err := filepath.Abs(c.Args().First())
	checkErr(err)

	imageProps := lib.ImageProps{
		Vertical:   c.Float64(vertical),
		Horizontal: c.Float64(horizontal),
		Top:        c.Int(top),
		Bottom:     c.Int(bottom),
		Left:       c.Int(left),
		Right:      c.Int(right),
		Background: c.String(background)}

	monitors, err := lib.GetMonitors(false, false)
	checkErr(err)

	if len(monitors) == 0 {
		log.Println("No monitors detected.")
		return nil
	}

	previewWallpaperUsingFakeCache(w, imageProps, monitors)

	// Windows will fail to read the wallpapers if we delete them too fast
	// lib.ShouldWait() ?
	if runtime.GOOS == "windows" {
		<-time.After(5 * time.Second)
	}
	return nil
}

// Redirects the cache to a temporary folder or panics
func redirectCache() {
	conf, err := lib.GetConfig()
	checkErr(err)

	tdir, err := lib.TempDir()
	checkErr(err)

	// lie about where the cache is
	conf.CacheDirectory = filepath.Join(tdir, "cache")
}

func previewWallpaperUsingFakeCache(
	wallpaper string, imageProps lib.ImageProps, monitors []*lib.Monitor) {

	redirectCache()

	outFiles := make([]string, len(monitors))

MonitorLoop:
	for i, m := range monitors {
		cachePng, err := lib.GetCacheImagePath("preview", m, imageProps)
		checkErr(err)

		outFiles[i] = cachePng + ".bmp"

		m.Wallpaper = outFiles[i]

		for _, s := range outFiles[:i] {
			if outFiles[i] == s {
				continue MonitorLoop
			}
		}

		should, err := lib.ShouldProcessImage(wallpaper, outFiles[i])
		checkErr(err)
		if !should {
			continue MonitorLoop
		}

		po := lib.ProcessOptions{
			Input:      wallpaper,
			Output:     outFiles[i],
			Width:      m.Width,
			Height:     m.Height,
			Denoise:    true,
			Flatten:    true,
			CropOrPad:  true,
			ImageProps: imageProps}

		err = lib.ProcessImage(po)
		checkErr(err)
	}

	err := lib.SetMonitorWallpapers(monitors)
	checkErr(err)
}
