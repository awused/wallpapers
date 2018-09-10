// +build !windows

package changewallpaperlib

import (
	"errors"
	"fmt"
	"io/ioutil"
	"os"
	"os/exec"
	"os/user"
	"path/filepath"
	"strings"
)

// Clean up a string for use as part of a file or directory name
// Periods would be perfectly safe but I'd rather not have them inside names
// var repeatedHyphens = regexp.MustCompile(`--+`)
// var safeFilenameRegex = regexp.MustCompile(`[^\p{L}\p{N}-_+=]+`)
//
// func appNameToFilename(input string) string {
// 	output := strings.ToLower(input)
// 	output = safeFilenameRegex.ReplaceAllString(output, "-")
// 	output = repeatedHyphens.ReplaceAllString(output, "-")
// 	return strings.Trim(output, "-")
// }

const dbusAddress = "DBUS_SESSION_BUS_ADDRESS"

func setDBUSAddress() error {
	dbus := os.Getenv(dbusAddress)
	if dbus == "" {
		// For now just assume we're dealing with per-user dbus sessions
		// TODO -- This is definitely not good enough
		user, err := user.Current()
		if err != nil {
			return nil
		}
		uid := user.Uid
		if uid == "" {
			return errors.New("No $UID set")
		}
		return os.Setenv(dbusAddress, "unix:path=/run/user/"+uid+"/bus")
	}

	return nil
}

// BMP takes a lot of space but PNG takes non-trivial CPU time
const outputFormat = "bmp"

func getNextOutputFile(c *Config) (AbsolutePath, error) {
	dir, err := filepath.Abs(c.OutputDir)
	if err != nil {
		return "", err
	}

	f, err := ioutil.TempFile(dir, "*."+outputFormat)
	if f != nil {
		err = f.Close()
	}
	if err != nil {
		return "", err
	}

	// TODO -- Remove this stupid dance once everything moves to Go 1.11's better TempFile
	if filepath.Ext(f.Name()) != "."+outputFormat {
		err = os.Remove(f.Name())
		if err != nil {
			return "", err
		}
		return f.Name() + "." + outputFormat, nil
	}

	return f.Name(), nil
}

func setGnomeWallpaper(wallpaper AbsolutePath, c *Config) error {
	err := os.MkdirAll(c.OutputDir, 0755)
	if err != nil {
		return fmt.Errorf(
			"Error creating OutputDir [%s]: %s", c.OutputDir, err)
	}

	oldWall, err := runBash(`
		gsettings get org.gnome.desktop.background picture-uri
	`)
	if err != nil {
		return err
	}
	_, err = runBash(`
		gsettings set org.gnome.desktop.background picture-options spanned
		gsettings set org.gnome.desktop.background picture-uri "file://` + wallpaper + `"
	`)
	if err != nil {
		return err
	}

	oldWall = strings.TrimPrefix(strings.Trim(oldWall, "'\n"), "file://")
	// Only remove files we own
	if filepath.Dir(oldWall) == c.OutputDir {
		// This could have alread been removed, bury any errors
		_ = os.Remove(oldWall)
	}

	return nil
}

func SetMonitorWallpapers(monitors []*Monitor) error {
	if len(monitors) == 0 {
		return nil
	}

	c, err := GetConfig()
	if err != nil {
		return err
	}

	// Right now we know we're dealing with one single session that is gnome, so don't even check
	if true {
		os.Setenv("DISPLAY", monitors[0].session.display)
		err = setDBUSAddress()
		if err != nil {
			return err
		}

		wallpaper, err := getNextOutputFile(c)
		if err != nil {
			return err
		}
		err = combineImages(monitors, wallpaper)
		if err != nil {
			return err
		}

		return setGnomeWallpaper(wallpaper, c)
	}

	return errors.New("Not yet implemented")
}

// Might need to change the API or call it after getting sessions
func CheckIfLocked() (bool, error) {
	return false, nil
}

// No-op
func AttachParentConsole() {}

func runBash(cmd string) (string, error) {
	// See http://redsymbol.net/articles/unofficial-bash-strict-mode/
	command := `
		set -euo pipefail
		IFS=$'\n\t'
		` + cmd + "\n"

	bash := exec.Command("/usr/bin/env", "bash")
	bash.Stdin = strings.NewReader(command)
	bash.Stderr = os.Stderr

	bashOut, err := bash.Output()
	return string(bashOut), err
}
