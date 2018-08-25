package main

import (
	"log"
	"os"

	lib "github.com/awused/wallpapers/lib"
	"github.com/urfave/cli"
)

func main() {
	c, err := lib.Init()
	checkErr(err)
	defer lib.Cleanup()

	if c.LogFile != "" {
		f, err := os.OpenFile(c.LogFile, os.O_RDWR|os.O_CREATE|os.O_APPEND, 0666)
		if err != nil {
			log.Fatalf("Error opening log file: %v", err)
		}
		defer f.Close()

		log.SetOutput(f)
	}

	app := cli.NewApp()
	app.Usage = "Program for managing wallpapers for multiple monitors"

	app.Commands = []cli.Command{
		previewCommand(),
		syncCommand(),
		randomCommand(),
	}

	err = app.Run(os.Args)
	checkErr(err)
}

func checkErr(err error) {
	if err != nil {
		log.Println(err)
		panic(err)
	}
}
