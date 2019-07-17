package changewallpaperlib

import (
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"strconv"
	"sync"

	"github.com/BurntSushi/toml"
	"github.com/awused/awconf"
)

type Config struct {
	DatabaseDir             string
	TempDirectory           string
	OutputDir               string
	LogFile                 string
	Waifu2xCaffe            *string
	Waifu2xNCNNVulkan       *string
	Waifu2xNCNNVulkanModels string
	Waifu2xCPP              string
	ForceOpenCL             bool
	Waifu2xCPPModels        string
	ImageMagick7            bool
	ImageMagick             string
	OriginalsDirectory      string
	CacheDirectory          string
	ImageFileExtensions     []string
	MaxPNGWallpaperSize     int64
	CPUScale                bool
	CPUThreads              *int
}

var props map[string]map[string]map[string]ImageProps
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

func GetConfigImageProps(path RelativePath, m *Monitor) ImageProps {
	if conf == nil || m == nil || props == nil {
		return ImageProps{}
	}

	slashPath := filepath.ToSlash(path)

	return props[slashPath][m.aspectX][m.aspectY]
}

func aspectRatio(m *Monitor) (string, string) {
	a, b := m.Width, m.Height

	for b != 0 {
		a, b = b, a%b
	}

	return strconv.Itoa(m.Width / a), strconv.Itoa(m.Height / a)
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

	if c.DatabaseDir == "" {
		return fmt.Errorf("Config missing DatabaseDir")
	}

	fi, err := os.Stat(c.DatabaseDir)
	if err != nil {
		return fmt.Errorf(
			"Error calling os.Stat on DatabaseDir [%s]: %s", c.DatabaseDir, err)
	}
	if !fi.IsDir() {
		return fmt.Errorf("DatabaseDir [%s] is not a directory", c.DatabaseDir)
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

	if c.Waifu2xCaffe == nil && c.Waifu2xCPP == "" && c.Waifu2xNCNNVulkan == nil {
		return fmt.Errorf("Config missing any option for waifu2x")
	}

	if c.Waifu2xCaffe != nil && *c.Waifu2xCaffe == "" {
		return fmt.Errorf("Config contains empty Waifu2xCaffe")
	}

	if c.Waifu2xNCNNVulkan != nil && *c.Waifu2xNCNNVulkan == "" {
		return fmt.Errorf("Config contains empty Waifu2xNCNNVulkan")
	}

	if c.CPUThreads != nil && *c.CPUThreads <= 0 {
		return fmt.Errorf("CPUThreads must be greater than 0")
	}

	if c.ImageMagick == "" {
		if c.ImageMagick7 {
			c.ImageMagick = "magick"
		} else {
			c.ImageMagick = "convert"
		}
	}

	if c.OriginalsDirectory == "" {
		return fmt.Errorf("Config missing Originals directory")
	}
	fi, err = os.Stat(c.OriginalsDirectory)
	if err != nil {
		return fmt.Errorf(
			"Error calling os.Stat on OriginalsDirectory [%s]: %s", c.OriginalsDirectory, err)
	}
	if !fi.IsDir() {
		return fmt.Errorf("OriginalsDirectory [%s] is not a directory", c.OriginalsDirectory)
	}

	propsPath := filepath.Join(c.OriginalsDirectory, ".properties.toml")
	_, err = os.Stat(propsPath)
	if err == nil {
		_, err = toml.DecodeFile(propsPath, &props)
		if err != nil {
			return err
		}
	} else if !os.IsNotExist(err) {
		return fmt.Errorf("Unexpected error %s when opening [%s]", err, propsPath)
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

	if c.OutputDir == "" {
		c.OutputDir = filepath.Join(os.Getenv("HOME"), ".wallpapers")
	}

	fi, err = os.Stat(c.OutputDir)
	if err == nil && !fi.IsDir() {
		return fmt.Errorf("OutputDir [%s] is a regular file", c.OutputDir)
	} else if err != nil {
		if !os.IsNotExist(err) {
			return fmt.Errorf(
				"Error calling os.Stat on OutputDir [%s]: %s", c.OutputDir, err)
		}
	}

	if len(c.ImageFileExtensions) == 0 {
		return fmt.Errorf("No ImageFileExtensions present in config")
	}

	return nil
}
