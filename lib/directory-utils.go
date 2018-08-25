package changewallpaperlib

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

type InputFiles struct {
	Root        string
	Files, Dirs []string
}

// TODO -- make these fully separate types instead of just aliases
type RelativePath = string
type AbsolutePath = string

func GetAllOriginals() (relPaths []RelativePath, err error) {
	c, err := GetConfig()
	if err != nil {
		return
	}
	dir := c.OriginalsDirectory

	err = filepath.Walk(dir, func(path string, f os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		if !strings.HasPrefix(path, dir) {
			return fmt.Errorf("Unexpected path")
		}

		if f.Mode().IsRegular() {
			ext := strings.TrimLeft(strings.ToLower(filepath.Ext(path)), ".")
			for _, t := range c.ImageFileExtensions {
				if t == ext {
					path = strings.TrimPrefix(path, dir)
					path = strings.TrimPrefix(path, string(filepath.Separator))
					relPaths = append(relPaths, path)
					break
				}
			}
		}

		return nil
	})
	if err != nil {
		return
	}

	return
}

// Returns true if outFile doesn't exist or if inFile was modified more recently
func ShouldProcessImage(inFile, outFile AbsolutePath) (bool, error) {
	ofi, err := os.Stat(outFile)
	if err != nil {
		if os.IsNotExist(err) {
			return true, nil
		}
		return false, err
	}

	ifi, err := os.Stat(inFile)
	if err != nil {
		return false, err
	}

	return ofi.ModTime().Before(ifi.ModTime()), nil
}

func getMonitorCacheDirectory(cache string, m *Monitor) string {
	return filepath.Join(cache, fmt.Sprintf("%dx%d", m.Width, m.Height))
}

func GetFullInputPath(relPath RelativePath) (AbsolutePath, error) {
	c, err := GetConfig()
	if err != nil {
		return "", err
	}
	return filepath.Abs(filepath.Join(c.OriginalsDirectory, relPath))
}

func GetCacheImagePath(relPath RelativePath, m *Monitor, co CropOffset) (
	AbsolutePath, error) {

	c, err := GetConfig()
	if err != nil {
		return "", err
	}

	// This will result in double extensions (".png.png" or ".jpg.png")
	// This is necessary to avoid name collisions since everything becomes png
	return filepath.Abs(filepath.Join(
		getMonitorCacheDirectory(c.CacheDirectory, m), relPath+co.String()+".png"))
}
