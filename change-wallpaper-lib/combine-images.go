package changewallpaperlib

import (
	"fmt"
	"os/exec"
	"syscall"
)

// Assumes it is being passed images that have already been scaled for the monitors
func CombineImages(images []string, monitors []*Monitor, outFile string) error {
	c, err := GetConfig()
	if err != nil {
		return err
	}

	if len(monitors) != len(images) {
		return fmt.Errorf("Number of input images does not match number of monitors")
	}

	if len(monitors) == 0 {
		return nil
	}

	right := monitors[0].Right
	left := monitors[0].Left
	top := monitors[0].Top
	bottom := monitors[0].Bottom

	for _, m := range monitors {
		if m.Right > right {
			right = m.Right
		}
		if m.Left < left {
			left = m.Left
		}
		if m.Top < top {
			top = m.Top
		}
		if m.Bottom > bottom {
			bottom = m.Bottom
		}
	}

	comps := make([]string, len(images)*4)

	for i, m := range monitors {
		comps[i*4] = images[i]
		comps[i*4+1] = "-geometry"
		comps[i*4+2] = fmt.Sprintf("%+d%+d", m.Left-left, m.Top-top)
		comps[i*4+3] = "-composite"
	}

	args := []string{"convert", "-size", fmt.Sprintf("%dx%d", right-left, bottom-top), "xc:white"}
	args = append(args, comps...)
	args = append(args, outFile)

	cmd := exec.Command(c.ImageMagick, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Run()
}
