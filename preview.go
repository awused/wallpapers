package main

import (
	"errors"
	"path/filepath"
	"time"

	lib "github.com/awused/wallpapers/lib"
	"github.com/urfave/cli"
)

const horizontal = "horizontal"
const vertical = "vertical"
const top = "top"
const bottom = "bottom"
const left = "left"
const right = "right"
const background = "background"

func previewCommand() cli.Command {
	cmd := cli.Command{}
	cmd.Name = "preview"
	cmd.Usage = "Preview a single wallpaper on every monitor"
	cmd.ArgsUsage = "FILE"
	cmd.Before = beforeFunc
	cmd.Flags = []cli.Flag{
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
		cli.IntFlag{
			Name:  top + ", t",
			Value: 0,
			Usage: "Pixels to crop off the top, negative values pad",
		},
		cli.IntFlag{
			Name:  bottom + ", b",
			Value: 0,
			Usage: "Pixels to crop off the bottom, negative values pad",
		},
		cli.IntFlag{
			Name:  left + ", l",
			Value: 0,
			Usage: "Pixels to crop off the left side, negative values pad",
		},
		cli.IntFlag{
			Name:  right + ", r",
			Value: 0,
			Usage: "Pixels to crop off the right side, negative values pad",
		},
		cli.StringFlag{
			Name:  background + ", bg",
			Value: "black",
			Usage: "Background colour to use when padding",
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

	previewWallpaperUsingFakeCache(w, imageProps)

	// Windows will fail to read the wallpapers if we delete them too fast
	<-time.After(5 * time.Second)
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
	wallpaper string, imageProps lib.ImageProps) {

	redirectCache()

	monitors, err := lib.GetMonitors()
	checkErr(err)

	outFiles := make([]string, len(monitors))
	scaledFiles := make([]string, len(monitors))

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

		scaledFiles[i], err = lib.GetScaledIntermediateFile(po)
		checkErr(err)

		for _, sf := range scaledFiles[:i] {
			if scaledFiles[i] == sf {
				po.Input = sf
				// Scaled files have already been denoised and cropped/padded
				po.Denoise = false
				po.CropOrPad = false
				break
			}
		}

		err = lib.ProcessImage(po)
		checkErr(err)
	}

	err = lib.SetMonitorWallpapers(monitors)
	checkErr(err)
}
