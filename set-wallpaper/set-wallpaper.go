package main

import (
	"log"
	"os"

	lib "github.com/awused/windows-wallpapers/change-wallpaper-lib"
)

const errorLog = `C:\Logs\set-wallpaper-error.log`

func main() {
	f, err := os.OpenFile(errorLog, os.O_RDWR|os.O_CREATE|os.O_APPEND, 0666)
	if err != nil {
		log.Fatalf("Error opening file: %v", err)
	}
	defer f.Close()

	log.SetOutput(f)

	_, err = lib.ReadConfig()
	if err != nil {
		log.Fatal(err)
	}

	err = lib.ChangeBackground()
	if err != nil {
		log.Fatal(err)
	}
}
