package changewallpaperlib

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

type InputDirectory struct {
	Root        string
	Prefix      string
	Files, Dirs []string
}

func WalkAllInputDirectories() (ret []*InputDirectory, err error) {
	c, err := GetConfig()
	if err != nil {
		return
	}

	ret = make([]*InputDirectory, len(c.OriginalsDirectories))

	for i, d := range c.OriginalsDirectories {
		ret[i] = &InputDirectory{Root: d.Path, Prefix: d.HashPrefix}
		err = filepath.Walk(d.Path, func(path string, f os.FileInfo, err error) error {
			if err != nil {
				return err
			}

			if !strings.HasPrefix(path, d.Path) {
				return fmt.Errorf("Unexpected path")
			}

			path = strings.TrimPrefix(path, d.Path)

			if f.Mode().IsRegular() {
				pathLower := strings.ToLower(path)
				for _, t := range c.ImageFileExtensions {
					if strings.HasSuffix(pathLower, t) {
						ret[i].Files = append(ret[i].Files, path)
						break
					}
				}
			} else if f.Mode().IsDir() {
				ret[i].Dirs = append(ret[i].Dirs, path)
			}

			return nil
		})
		if err != nil {
			return
		}
	}

	return
}

// Returns true if outFile doesn't exist or if inFile was modified more recently
func ShouldProcessImage(inFile, outFile string) (bool, error) {
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

func SetupCacheDirectories(ms []*Monitor) error {
	c, err := GetConfig()
	if err != nil {
		return err
	}

	for _, m := range ms {
		dir := getMonitorCacheDirectory(c.CacheDirectory, m)
		fi, err := os.Stat(dir)
		if err != nil {
			if os.IsNotExist(err) {
				err = os.Mkdir(dir, 0)
			}

			if err != nil {
				return err
			}
		} else {
			if !fi.Mode().IsDir() {
				return fmt.Errorf("Regular file exists at same path as cache directory [%s]", dir)
			}
		}
	}
	return nil
}

func GetFullInputPath(dir *InputDirectory, relPath string) (string, error) {
	return filepath.Abs(filepath.Join(dir.Root, relPath))
}

func GetCacheImagePath(dir *InputDirectory, relPath string, m *Monitor) (string, error) {
	c, err := GetConfig()
	if err != nil {
		return "", err
	}

	h := sha256.Sum256([]byte(MakeUniqueIdForFile(dir, relPath)))
	return filepath.Join(getMonitorCacheDirectory(c.CacheDirectory, m), hex.EncodeToString(h[:])+".png"), nil
}

func MakeUniqueIdForFile(dir *InputDirectory, relPath string) string {
	return dir.Prefix + filepath.ToSlash(relPath)
}
