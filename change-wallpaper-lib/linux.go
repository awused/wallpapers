// +build !windows

package changewallpaperlib

import (
	"errors"
	"syscall"
)

type Monitor struct {
	Width     int
	Height    int
	Path      string
	Wallpaper AbsolutePath
}

var sysProcAttr = &syscall.SysProcAttr{}

// Returns a a list of monitors sorted by descending pixel count
// Doing the largest monitors first assists with parallelization
func GetMonitors() ([]*Monitor, error) {
	return nil, errors.New("Not yet implemented")
}

func SetMonitorWallpapers(monitors []*Monitor) error {
	return errors.New("Not yet implemented")
}
