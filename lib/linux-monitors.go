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
	"github.com/BurntSushi/xgb/xinerama"
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

	// really should not fail
	_ = os.Unsetenv("COLUMNS")

	displays, err := runBash(
		`ps e -u "$USER" | sed -rn 's/.* DISPLAY=(:[0-9]+).*/\1/p' | uniq`)
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

// TODO -- refactor this so it's called inside GetMonitors and filters them
func checkIfLocked(s *session) bool {

	// per-user dbus detected
	if true {
		setDBUSAddress()
	}

	// TODO -- refactor this properly.
	// Check for i3lock first
	if true {
		out, err := runBash(`
			pgrep -u $USER i3lock || test $? = 1
		`)

		if err != nil {
			return false
		}

		if strings.TrimSpace(out) != "" {
			return true
		}
	}

	// Again assuming GNOME
	if true {
		out, err := runBash(`
	gdbus call -e -d org.gnome.ScreenSaver -o /org/gnome/ScreenSaver -m org.gnome.ScreenSaver.GetActive | sed -e 's/[^a-zA-Z]//g'
	`)
		// We do not care about errors here. Assume it's unlocked
		if err == nil {
			return false
		}
		return strings.TrimSpace(out) == "true"
	}

	return false
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

// Ignore errors here, fail open
func checkFullscreenX(X *xgbutil.XUtil) bool {
	windows, _ := ewmh.ClientListGet(X)

	for _, w := range windows {
		states, _ := ewmh.WmStateGet(X, w)
		for _, state := range states {
			if state == "_NET_WM_STATE_FULLSCREEN" {
				return true
			}
		}
	}

	return false
}

func getXSessionData(s *session, unlocked bool, nofs bool) ([]*Monitor, error) {
	monitors := []*Monitor{}
	X, err := xgbutil.NewConnDisplay(s.display)
	if err != nil {
		return nil, err
	}
	Xgb := X.Conn()

	if unlocked && checkIfLocked(s) {
		return nil, nil
	}

	if nofs && checkFullscreenX(X) {
		return nil, nil
	}

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

	err = xinerama.Init(Xgb)
	if err != nil {
		return nil, err
	}

	reply, err := xinerama.QueryScreens(Xgb).Reply()
	if err != nil {
		return nil, err
	}

	for _, info := range reply.ScreenInfo {
		m := Monitor{session: s}
		m.left = int(info.XOrg)
		m.top = int(info.YOrg)
		m.Width = int(info.Width)
		m.Height = int(info.Height)
		monitors = append(monitors, &m)
	}

	return monitors, nil
}

func monitorsForSession(s *session, unlocked bool, nofs bool) ([]*Monitor, error) {
	monitors := []*Monitor{}
	if s.sType == xType {
		ms, err := getXSessionData(s, unlocked, nofs)
		if err != nil {
			return nil, err
		}

		monitors = append(monitors, ms...)
	}

	return monitors, nil
}

func GetMonitors(unlocked bool, nofs bool) ([]*Monitor, error) {
	// Stop polluting stdout
	xgb.Logger.SetOutput(ioutil.Discard)
	xgbutil.Logger.SetOutput(ioutil.Discard)

	sessions, err := listSessions()
	if err != nil {
		return nil, err
	}

	monitors := []*Monitor{}
	for _, s := range sessions {
		ms, err := monitorsForSession(&s, unlocked, nofs)
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
