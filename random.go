package main

import (
	"fmt"
	"log"

	"github.com/awused/go-strpick/persistent"
	lib "github.com/awused/wallpapers/lib"
	"github.com/urfave/cli"
)

const unlocked = "unlocked"
const nofullscreen = "no-fullscreen"

func randomCommand() cli.Command {
	cmd := cli.Command{}
	cmd.Name = "random"
	cmd.Usage = "Randomly select a wallpaper for each monitor"
	cmd.Before = beforeFunc
	cmd.Flags = []cli.Flag{
		cli.BoolFlag{
			Name:  unlocked + ", u",
			Usage: "Checks to see if the screen is unlocked and aborts if it is",
		},
		cli.BoolFlag{
			Name: nofullscreen + ", n",
			Usage: "Checks to see if there are any full screen applications and " +
				"aborts if there are",
		},
	}

	cmd.Action = randomAction

	return cmd
}

func randomAction(c *cli.Context) error {
	conf, err := lib.GetConfig()
	checkErr(err)

	monitors, err := lib.GetMonitors(c.Bool(unlocked), c.Bool(nofullscreen))
	checkErr(err)

	if len(monitors) == 0 {
		if !c.Bool(unlocked) && !c.Bool(nofullscreen) {
			log.Println("No monitors detected.")
		}
		return nil
	}

	picker, err := persistent.NewPicker(conf.DatabaseDir)
	checkErr(err)
	defer picker.Close()

	originals, err := lib.GetAllOriginals()
	checkErr(err)

	err = picker.AddAll(originals)
	checkErr(err)

	sz, err := picker.Size()
	checkErr(err)
	if sz == 0 {
		// Also log to stdout since this doesn't panic
		fmt.Println("No wallpapers present in OriginalDirectory")
		if conf.LogFile != "" {
			log.Println("No wallpapers present in OriginalDirectory")
		}
		return nil
	}

	inputRelPaths, err := picker.TryUniqueN(len(monitors))
	checkErr(err)

	for i, relPath := range inputRelPaths {
		m := monitors[i]
		imageProps := lib.GetConfigImageProps(relPath, m)

		absPath, err := lib.GetFullInputPath(relPath)
		checkErr(err)

		cachedFile, err := lib.GetCacheImagePath(relPath, m, imageProps)
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
				ImageProps: imageProps}
			err = lib.ProcessImage(po)
			checkErr(err)
		}

		m.Wallpaper = cachedFile
	}

	err = lib.SetMonitorWallpapers(monitors)
	checkErr(err)
	return nil
}
