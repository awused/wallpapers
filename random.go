package main

import (
	"encoding/json"
	"fmt"
	"log"
	"net"
	"path/filepath"

	"github.com/awused/go-strpick/persistent"
	lib "github.com/awused/wallpapers/lib"
	"github.com/urfave/cli/v2"
)

const unlocked = "unlocked"
const nofullscreen = "no-fullscreen"
const nompv = "no-mpv"

func randomCommand() *cli.Command {
	cmd := &cli.Command{}
	cmd.Name = "random"
	cmd.Usage = "Randomly select a wallpaper for each monitor"
	cmd.Before = beforeFunc
	cmd.Flags = []cli.Flag{
		&cli.BoolFlag{
			Name:    unlocked,
			Aliases: []string{"u"},
			Usage:   "Checks to see if the screen is unlocked and aborts if it is",
		},
		&cli.BoolFlag{
			Name:    nofullscreen,
			Aliases: []string{"n"},
			Usage: "Checks to see if there are any full screen applications and " +
				"aborts if there are",
		},
		&cli.StringFlag{
			Name: nompv,
			Usage: "Checks to see if an instance of MPV is running unpaused on " +
				"the given input-ipc-server and aborts if one is found. Only works " +
				"with the last MPV instance started on that ipc-server",
			Value: "",
		},
	}

	cmd.Action = randomAction

	return cmd
}

func randomAction(c *cli.Context) error {
	if checkMpv(c.String(nompv)) {
		return nil
	}

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
	checkErr(picker.SetRandomlyDistributeNewStrings(true))

	originals, err := lib.GetAllOriginals()
	checkErr(err)

	for i, s := range originals {
		originals[i] = filepath.ToSlash(s)
	}

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

	for i, s := range inputRelPaths {
		inputRelPaths[i] = filepath.FromSlash(s)
	}

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

type statusResponse struct {
	Data bool `json:"data"`
}

func checkMpv(socket string) bool {
	if socket == "" {
		return false
	}

	c, err := net.Dial("unix", socket)
	if err != nil {
		return false
	}
	defer c.Close()

	_, err = c.Write([]byte("{ \"command\": [\"get_property\", \"pause\"] }\n"))
	if err != nil {
		return false
	}

	out := make([]byte, 128)
	n, err := c.Read(out)
	if n == 0 || err != nil {
		return false
	}

	resp := statusResponse{}
	err = json.Unmarshal(out[:n], &resp)
	if err != nil {
		return false
	}

	return !resp.Data
}
