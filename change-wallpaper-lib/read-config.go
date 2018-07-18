package changewallpaperlib

import (
	"fmt"
	"os"

	"github.com/awused/awconf"
)

type OriginalsDirectory struct {
	Path       string
	HashPrefix string
}

type Config struct {
	WallpaperFile        string
	UsedWallpapersDBDir  string
	TempDirectory        string
	Waifu2x              string
	ImageMagick          string
	OriginalsDirectories []OriginalsDirectory
	CacheDirectory       string
	ImageFileExtensions  []string
	MaxPNGWallpaperSize  int64
}

var singleton *Config

func GetConfig() (*Config, error) {
	if singleton != nil {
		return singleton, nil
	}

	return nil, fmt.Errorf("ReadConfig never called")
}

func ReadConfig() (*Config, error) {
	c := &Config{}

	if err := awconf.LoadConfig("wallpapers", c); err != nil {
		return nil, err
	}

	singleton = c
	return c, c.validate()
}

func (c *Config) validate() error {
	if c.WallpaperFile == "" {
		return fmt.Errorf("Config missing WallpaperFile")
	}

	fi, err := os.Stat(c.WallpaperFile)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	if !os.IsNotExist(err) && !fi.Mode().IsRegular() {
		return fmt.Errorf("WallpaperFile [%s] is not a regular file", c.WallpaperFile)
	}

	if c.UsedWallpapersDBDir == "" {
		return fmt.Errorf("Config missing UsedWallpapersDBDir")
	}

	fi, err = os.Stat(c.UsedWallpapersDBDir)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	if !os.IsNotExist(err) && fi.Mode().IsRegular() {
		return fmt.Errorf("UsedWallpapersDBDir [%s] is not a directory", c.UsedWallpapersDBDir)
	}

	if c.TempDirectory == "" {
		return fmt.Errorf("Config missing TempDirectory")
	}

	fi, err = os.Stat(c.TempDirectory)
	if err != nil {
		return err
	}
	if !fi.IsDir() {
		return fmt.Errorf("TempDirectory [%s] is not a directory", c.TempDirectory)
	}

	if c.Waifu2x == "" {
		return fmt.Errorf("Config missing path to Waifu2x-caffe")
	}

	fi, err = os.Stat(c.Waifu2x)
	if err != nil {
		return err
	}
	if !fi.Mode().IsRegular() {
		return fmt.Errorf("Waifu2x executable [%s] is not a regular file", c.Waifu2x)
	}

	if c.ImageMagick == "" {
		return fmt.Errorf("Config missing ImageMagick command")
	}

	if len(c.OriginalsDirectories) == 0 {
		return fmt.Errorf("No OriginalsDirectories present in config")
	}

	prefixes := make(map[string]bool)

	for _, d := range c.OriginalsDirectories {
		if d.Path == "" {
			return fmt.Errorf("Empty Path present in OriginalsDirectories")
		}

		if d.HashPrefix == "" {
			return fmt.Errorf("Empty HashPrefix present in OriginalsDirectories")
		}

		if _, ok := prefixes[d.HashPrefix]; ok {
			return fmt.Errorf("Duplicate HashPrefix [%s] present in OriginalsDirectories", d.HashPrefix)
		}

		prefixes[d.HashPrefix] = true

		fi, err = os.Stat(d.Path)
		if err != nil {
			return err
		}
		if !fi.IsDir() {
			return fmt.Errorf("OriginalsDirectory [%s] is not a directory", d.Path)
		}
	}

	if c.CacheDirectory == "" {
		return fmt.Errorf("Config missing CacheDirectory")
	}

	fi, err = os.Stat(c.CacheDirectory)
	if err != nil {
		return err
	}
	if !fi.IsDir() {
		return fmt.Errorf("CacheDirectory [%s] is not a directory", c.CacheDirectory)
	}

	if len(c.ImageFileExtensions) == 0 {
		return fmt.Errorf("No OriginalsDirectories present in config")
	}

	return nil
}
