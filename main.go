package main

import (
	"log"
	"os"

	lib "github.com/awused/wallpapers/lib"
	"github.com/urfave/cli/v2"
)

func main() {
	lib.AttachParentConsole()
	defer lib.Cleanup()

	app := cli.NewApp()
	app.Usage = "Program for managing wallpapers for multiple monitors"
	app.Commands = []*cli.Command{
		previewCommand(),
		syncCommand(),
		randomCommand(),
		interactiveCommand(),
	}

	err := app.Run(os.Args)
	checkErr(err)
}

// Only init when necessary
// Can't do conditionally in app.Before because app.Before is useless for any purpose
func beforeFunc(ctxt *cli.Context) error {
	c, err := lib.Init()
	checkErr(err)

	if c.LogFile != "" {
		f, err := os.OpenFile(c.LogFile, os.O_RDWR|os.O_CREATE|os.O_APPEND, 0666)
		if err != nil {
			log.Fatalf("Error opening log file: %v", err)
		}
		defer f.Close()

		log.SetOutput(f)
	}
	return nil
}

func checkErr(err error) {
	if err != nil {
		log.Println(err)
		panic(err)
	}
}
