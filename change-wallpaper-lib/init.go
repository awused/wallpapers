package changewallpaperlib

import (
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"strconv"
	"sync"

	"github.com/awused/awconf"
)

type Config struct {
	UsedWallpapersDBDir string
	TempDirectory       string
	Waifu2xCaffe        *string
	Waifu2xCPP          string
	ImageMagick         string
	OriginalsDirectory  string
	CacheDirectory      string
	ImageFileExtensions []string
	MaxPNGWallpaperSize int64
	CPUScale            bool
	CPUThreads          *int
	Offset              map[string]map[string]map[string]CropOffset
}

var conf *Config

var tempDir string
var tempErr error
var tempOnce sync.Once

func TempDir() (string, error) {
	c, err := GetConfig()
	if err != nil {
		return "", err
	}

	tempOnce.Do(func() {
		tempDir, tempErr = ioutil.TempDir(c.TempDirectory, "wallpapers")
	})

	return tempDir, tempErr
}

func GetConfig() (*Config, error) {
	if conf != nil {
		return conf, nil
	}

	return nil, fmt.Errorf("Init never called")
}

func GetConfigCropOffset(path RelativePath, m *Monitor) CropOffset {
	if conf == nil || m == nil {
		return CropOffset{}
	}

	slashPath := filepath.ToSlash(path)
	x, y := aspectRatio(m)

	return conf.Offset[slashPath][x][y]
}

// Memoize normalized aspect ratio strings per monitor resolution
// Not even sure if this is worth doing, but the code has been written
var xMap = make(map[int]map[int]string)
var yMap = make(map[int]map[int]string)

func aspectRatio(m *Monitor) (string, string) {
	x, y := xMap[m.Width][m.Height], yMap[m.Width][m.Height]

	if x == "" {
		a, b := m.Width, m.Height

		for b != 0 {
			a, b = b, a%b
		}

		if _, ok := xMap[m.Width]; !ok {
			xMap[m.Width] = make(map[int]string)
			yMap[m.Width] = make(map[int]string)
		}
		xMap[m.Width][m.Height] = strconv.Itoa(m.Width / a)
		yMap[m.Width][m.Height] = strconv.Itoa(m.Height / a)

		x, y = xMap[m.Width][m.Height], yMap[m.Width][m.Height]
	}
	return x, y
}

// Be sure to defer Cleanup() after calling this
func Init() (*Config, error) {
	c := &Config{}

	if err := awconf.LoadConfig("wallpapers", c); err != nil {
		return nil, err
	}

	conf = c
	err := c.validate()
	if err != nil {
		return nil, err
	}

	return c, nil
}

func Cleanup() error {
	// tempDir is private and can't be set outside of this package
	if tempDir != "" {
		return os.RemoveAll(tempDir)
	}
	return nil
}

// For very long running processes that might be scaling thousands of files
// Call this periodically to empty the temporary directory
func PartialCleanup() error {
	if tempDir == "" {
		return nil
	}

	files, err := ioutil.ReadDir(tempDir)
	if err != nil {
		return err
	}

	for _, f := range files {
		err = os.Remove(filepath.Join(tempDir, f.Name()))
		if err != nil {
			return err
		}
	}
	return nil
}

func (c *Config) validate() error {
	/*if c.WallpaperFile == "" {
		return fmt.Errorf("Config missing WallpaperFile")
	}

	fi, err := os.Stat(c.WallpaperFile)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	if !os.IsNotExist(err) && !fi.Mode().IsRegular() {
		return fmt.Errorf("WallpaperFile [%s] is not a regular file", c.WallpaperFile)
	}*/

	if c.UsedWallpapersDBDir == "" {
		return fmt.Errorf("Config missing UsedWallpapersDBDir")
	}

	fi, err := os.Stat(c.UsedWallpapersDBDir)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	if !os.IsNotExist(err) && fi.Mode().IsRegular() {
		return fmt.Errorf("UsedWallpapersDBDir [%s] is not a directory", c.UsedWallpapersDBDir)
	}

	if c.TempDirectory != "" {
		fi, err = os.Stat(c.TempDirectory)

		if err != nil {
			return err
		}
		if !fi.IsDir() {
			return fmt.Errorf("TempDirectory [%s] is not a directory", c.TempDirectory)
		}
	}

	if c.Waifu2xCaffe == nil && c.Waifu2xCPP == "" {
		return fmt.Errorf("Config missing Waifu2xCaffe and Waifu2xCPP")
	}

	if c.Waifu2xCaffe != nil && *c.Waifu2xCaffe == "" {
		return fmt.Errorf("Config contains empty Waifu2xCaffe")
	}

	if c.CPUThreads != nil && *c.CPUThreads <= 0 {
		return fmt.Errorf("CPUThreads must be greater than 0")
	}

	if c.ImageMagick == "" {
		return fmt.Errorf("Config missing ImageMagick command")
	}

	if c.OriginalsDirectory == "" {
		return fmt.Errorf("Config missing Originals directory")
	}
	fi, err = os.Stat(c.OriginalsDirectory)
	if err != nil {
		return err
	}
	if !fi.IsDir() {
		return fmt.Errorf("OriginalsDirectory [%s] is not a directory", c.OriginalsDirectory)
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
