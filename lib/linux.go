// +build !windows

package changewallpaperlib

import (
	"errors"
	"syscall"
)

const WINDOWS = false

type Monitor struct {
	Width     int
	Height    int
	Path      string
	Wallpaper AbsolutePath
}

var sysProcAttr = &syscall.SysProcAttr{}

func GetMonitors() ([]*Monitor, error) {
	return nil, errors.New("Not yet implemented")
}

func SetMonitorWallpapers(monitors []*Monitor) error {
	return errors.New("Not yet implemented")
}

func CheckIfLocked() (bool, error) {
	return false, errors.New("Not yet implemented")
}

func AttachParentConsole() {}
