package main

import (
	"log"

	"github.com/awused/go-strpick/persistent"
	lib "github.com/awused/wallpapers/lib"
	"github.com/urfave/cli"
)

const unlocked = "unlocked"

func randomCommand() cli.Command {
	cmd := cli.Command{}
	cmd.Name = "random"
	cmd.Usage = "Randomly select a wallpaper for each monitor"
	cmd.Flags = []cli.Flag{
		cli.BoolTFlag{
			Name:  unlocked + ", u",
			Usage: "Checks to see if the screen is unlocked and aborts if it is",
		},
	}

	cmd.Action = randomAction

	return cmd
}

func randomAction(c *cli.Context) error {
	conf, err := lib.GetConfig()
	checkErr(err)

	picker, err := persistent.NewPicker(conf.DatabaseDir)
	checkErr(err)
	defer picker.Close()

	if c.Bool(unlocked) {
		locked, err := lib.CheckIfLocked()
		checkErr(err)
		if locked {
			// Silently exit, this isn't an error
			return nil
		}
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
	return nil
}
