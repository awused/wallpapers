// +build !windows

package changewallpaperlib

import (
	"fmt"
	"os/exec"
)

// Assumes it is being passed images that have already been scaled for the monitors
func combineImages(monitors []*Monitor, outFile string) error {
	c, err := GetConfig()
	if err != nil {
		return err
	}

	if len(monitors) == 0 {
		return nil
	}

	width := 0
	height := 0

	for _, m := range monitors {
		if m.left+m.Width > width {
			width = m.left + m.Width
		}
		if m.top+m.Height > height {
			height = m.top + m.Height
		}
	}

	comps := []string{}

	for _, m := range monitors {
		comps = append(comps,
			m.Wallpaper,
			"-geometry", fmt.Sprintf("%+d%+d", m.left, m.top),
			"-composite")
	}

	args := append(getBaseConvertArgs(c),
		"-size", fmt.Sprintf("%dx%d", width, height),
		"xc:white")
	args = append(args, comps...)
	args = append(args, outFile)

	cmd := exec.Command(c.ImageMagick, args...)
	cmd.SysProcAttr = sysProcAttr
	return cmd.Run()
}
