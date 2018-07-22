package main

import (
	"crypto/sha256"
	"encoding/hex"
	"log"
	"os"
	"path/filepath"

	lib "github.com/awused/windows-wallpapers/change-wallpaper-lib"
)

//const errorLog = `C:\Logs\resync-wallpaper-cache-error.log`

func main() {
	//f, err := os.OpenFile(errorLog, os.O_RDWR | os.O_CREATE | os.O_APPEND, 0666)
	//if err != nil {
	//   log.Fatalf("Error opening file: %v", err)
	//}
	//defer f.Close()

	//log.SetOutput(f)

	_, err := lib.Init()
	if err != nil {
		log.Fatal(err)
	}

	monitors, err := lib.GetMonitors()
	if err != nil {
		log.Fatal(err)
	}

	originals, err := lib.GetAllOriginals()
	if err != nil {
		log.Fatal(err)
	}

	for _, relPath := range originals {
		for _, m := range monitors {
			inputAbsPath, err := lib.GetFullInputPath(relPath)
			if err != nil {
				log.Fatal(err)
			}
			oldOutFile, err := lib.GetOldCacheImagePath(relPath, m)
			if err != nil {
				log.Fatal(err)
			}
			newOutFile, err := lib.GetCacheImagePath(relPath, m)
			if err != nil {
				log.Fatal(err)
			}

			// If the old file is invalid, don't rename
			doScale, err := lib.ShouldProcessImage(inputAbsPath, oldOutFile)
			if err != nil {
				log.Fatal(err.Error())
			}

			if doScale {
				continue
			}

			// New file is already valid, don't overwrite
			doScale, err = lib.ShouldProcessImage(inputAbsPath, newOutFile)
			if err != nil {
				log.Fatal(err.Error())
			}

			if !doScale {
				continue
			}

			log.Printf("Renaming [%s] to [%s]", oldOutFile, newOutFile)

			err = os.Rename(oldOutFile, newOutFile)
			if err != nil {
				log.Fatal(err)
			}
		}

	}
}

// TODO --Remove
func GetOldCacheImagePath(relPath RelativePath, m *Monitor) (AbsolutePath, error) {
	c, err := GetConfig()
	if err != nil {
		return "", err
	}

	h := sha256.Sum256([]byte(MakeOldUniqueIdForFile(relPath)))
	return filepath.Join(getMonitorCacheDirectory(c.CacheDirectory, m), hex.EncodeToString(h[:])+".png"), nil
}
func MakeOldUniqueIdForFile(relPath RelativePath) string {
	return "a/" + filepath.ToSlash(relPath)
}
