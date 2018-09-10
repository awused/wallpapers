package changewallpaperlib

import (
	"errors"
	"io/ioutil"
	"os"
	"strings"
	"syscall"

	"github.com/BurntSushi/xgb"
	"github.com/BurntSushi/xgbutil"
	"github.com/BurntSushi/xgbutil/xinerama"
)

type sessionType int
type environment int

const (
	xType sessionType = iota
	// wayland sessionType = iota
)

const (
	gnome environment = iota
)

type session struct {
	display string
	sType   sessionType
	env     environment
}

type Monitor struct {
	Width     int
	Height    int
	left      int
	top       int
	Wallpaper AbsolutePath
	// Potentially support multiple sessions in the future
	session *session
}

var sysProcAttr = &syscall.SysProcAttr{}

// Assumes a display ID of the form ":[0-9]+"
// True if it's definitely a local X session
func testXSession(session string) bool {
	_, err := os.Stat("/tmp/.X11-unix/X" + strings.TrimLeft(session, ":"))
	return err == nil
}

func getSessionType(display string) (sessionType, error) {
	if testXSession(display) {
		return xType, nil
	}
	return -1, errors.New("Unknown session type")
}

// TODO -- return more than one
// TODO -- don't pre-check for X
func listSessionIDs() ([]string, error) {
	// If $DISPLAY is set we just check to see if it's an X session
	d := os.Getenv("DISPLAY")
	if d != "" {
		if testXSession(d) {
			return []string{d}, nil
		} else {
			return nil, errors.New(
				"$DISPLAY refers to a non-X session. Wayland is not yet supported")
		}
	}

	displays, err := runBash(`w "$USER" | grep ' :[0-9]*' | awk '{print $2}'`)
	if err != nil {
		return nil, err
	}

	for _, d := range strings.Split(strings.TrimSpace(displays), "\n") {
		// TODO -- remove cheating here
		if testXSession(d) {
			return []string{d}, nil
		}
	}

	return nil, errors.New("No Supported sessions found")
}

func listSessions() ([]session, error) {
	ids, err := listSessionIDs()
	if err != nil {
		return nil, err
	}
	output := []session{}

	for _, id := range ids {
		s := session{display: id}
		t, err := getSessionType(id)
		if err != nil {
			return nil, err
		}
		s.sType = t

		// TODO -- Do something here
		s.env = gnome
		output = append(output, s)
	}
	return output, nil
}

func monitorsForSession(s *session) ([]*Monitor, error) {
	monitors := []*Monitor{}
	if s.sType == xType {
		X, err := xgbutil.NewConnDisplay(s.display)
		if err != nil {
			return nil, err
		}

		heads, err := xinerama.PhysicalHeads(X)
		if err != nil {
			return nil, err
		}

		for _, h := range heads {
			m := Monitor{session: s}
			m.left = h.X()
			m.top = h.Y()
			m.Width = h.Width()
			m.Height = h.Height()
			monitors = append(monitors, &m)
		}
	}

	return monitors, nil
}

func GetMonitors() ([]*Monitor, error) {
	// Stop polluting stdout
	xgb.Logger.SetOutput(ioutil.Discard)
	xgbutil.Logger.SetOutput(ioutil.Discard)

	sessions, err := listSessions()
	if err != nil {
		return nil, err
	}

	monitors := []*Monitor{}
	for _, s := range sessions {
		ms, err := monitorsForSession(&s)
		if err != nil {
			return nil, err
		}
		monitors = append(monitors, ms...)
	}
	return monitors, nil
}
