// +build !windows

package changewallpaperlib

import (
	"errors"
	"fmt"
	"io/ioutil"
	"os"
	"regexp"
	"strings"
	"syscall"

	"github.com/BurntSushi/xgb"
	"github.com/BurntSushi/xgb/randr"
	"github.com/BurntSushi/xgb/xproto"
	"github.com/BurntSushi/xgbutil"
	"github.com/BurntSushi/xgbutil/ewmh"
)

type sessionType int
type environment int

const (
	xType sessionType = iota
	// wayland sessionType = iota
)

const (
	gnome   environment = iota
	i3      environment = iota
	unknown environment = iota
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
	aspectX   string
	aspectY   string
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

var displayRE = regexp.MustCompile(`^:[0-9]+`)

// Trims individual screens out of an X11 (todo wayland) DISPLAY variable
func trimDisplay(display string) string {
	trimmed := displayRE.FindString(display)
	if trimmed != "" {
		return trimmed
	}
	return display
}

// TODO -- return more than one
// TODO -- don't pre-check for X
func listSessionIDs() ([]string, error) {
	// If $DISPLAY is set we just check to see if it's an X session
	d := trimDisplay(os.Getenv("DISPLAY"))
	if d != "" {
		if testXSession(d) {
			return []string{d}, nil
		} else {
			return nil, errors.New(
				"$DISPLAY refers to a non-X session. Wayland is not yet supported")
		}
	}

	displays, err := runBash(
		`w "$USER" | { grep ' :[0-9]*' || test $? = 1; } | awk '{print $2}'`)
	if err != nil {
		return nil, err
	}

	for _, d := range strings.Split(strings.TrimSpace(displays), "\n") {
		// TODO -- remove cheating here
		if testXSession(d) {
			return []string{d}, nil
		}
	}

	return nil, nil
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

		output = append(output, s)
	}
	return output, nil
}

func getXSessionData(s *session) ([]*Monitor, error) {
	monitors := []*Monitor{}
	X, err := xgbutil.NewConnDisplay(s.display)
	if err != nil {
		return nil, err
	}
	Xgb := X.Conn()

	wm, err := ewmh.GetEwmhWM(X)
	if err != nil {
		return nil, err
	}

	wm = strings.ToLower(wm)
	if strings.Contains(wm, "gnome") {
		s.env = gnome
	} else if wm == "i3" {
		s.env = i3
	} else {
		// Feh probably works
		fmt.Fprintf(os.Stderr, "Encountered unknown WM/DE: %s\n", wm)
		s.env = unknown
	}

	err = randr.Init(Xgb)
	if err != nil {
		return nil, err
	}

	root := xproto.Setup(Xgb).DefaultScreen(Xgb).Root

	resources, err := randr.GetScreenResources(Xgb, root).Reply()
	if err != nil {
		return nil, err
	}

	for _, crtc := range resources.Crtcs {
		info, err := randr.GetCrtcInfo(Xgb, crtc, 0).Reply()
		if err != nil {
			return nil, err
		}

		m := Monitor{session: s}
		m.left = int(info.X)
		m.top = int(info.Y)
		m.Width = int(info.Width)
		m.Height = int(info.Height)
		monitors = append(monitors, &m)
	}

	return monitors, nil
}

func monitorsForSession(s *session) ([]*Monitor, error) {
	monitors := []*Monitor{}
	if s.sType == xType {
		ms, err := getXSessionData(s)
		if err != nil {
			return nil, err
		}

		monitors = append(monitors, ms...)
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

	for _, m := range monitors {
		m.aspectX, m.aspectY = aspectRatio(m)
	}

	return monitors, nil
}
