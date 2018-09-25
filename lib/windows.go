// +build windows

package changewallpaperlib

import (
	"errors"
	"fmt"
	"log"
	"os"
	"syscall"
	"unsafe"

	ole "github.com/go-ole/go-ole"
	"golang.org/x/sys/windows/registry"
)

type monitor struct {
	left   int32
	top    int32
	right  int32
	bottom int32
}

type Monitor struct {
	Width     int
	Height    int
	path      string
	aspectX   string
	aspectY   string
	Wallpaper AbsolutePath
}

// DesktopWallpaper does not extend IDispatch so this needs to be done manually
type IDesktopWallpaperVtbl struct {
	QueryInterface            uintptr
	AddRef                    uintptr
	Release                   uintptr
	SetWallpaper              uintptr
	GetWallpaper              uintptr
	GetMonitorDevicePathAt    uintptr
	GetMonitorDevicePathCount uintptr
	GetMonitorRECT            uintptr
	SetBackgroundColor        uintptr
	GetBackgroundColor        uintptr
	SetPosition               uintptr
	GetPosition               uintptr
	SetSlideshow              uintptr
	GetSlideshow              uintptr
	SetSlideshowOptions       uintptr
	GetSlideshowOptions       uintptr
	AdvanceSlideshow          uintptr
	GetStatus                 uintptr
	Enable                    uintptr
}

// Pulled from headers
const CLSID = "{C2CF3110-460E-4fc1-B9D0-8A1C0C9CC4BD}"
const IID = "{B92B56A9-8B55-4E14-9A89-0199BBB6F93B}"
const DWPOS_CENTER = uintptr(0)

// Monitor is counted but isn't attached to the computer
const S_FALSE = uintptr(2147500037)

var sysProcAttr = &syscall.SysProcAttr{HideWindow: true}

var modole32 = syscall.NewLazyDLL("ole32.dll")
var coTaskMemFree = modole32.NewProc("CoTaskMemFree")

func GetMonitors(unlocked bool, nofs bool) ([]*Monitor, error) {
	if nofs {
		log.Println("--no-fullscreen is not yet supported on Windows")
	}

	if unlocked {
		locked, err := checkIfLocked()
		if err != nil {
			return nil, err
		}
		if locked {
			return nil, nil
		}
	}
	/*
		Old legacy code using one large wallpaper tiled across all monitors.

		var dpiLib = syscall.NewLazyDLL("shcore.dll")
		var dpiProc = dpiLib.NewProc("SetProcessDpiAwareness")

		ret, _, err := dpiProc.Call(uintptr(2))
		if ret != 0 {
			return nil, err
		}

		var userLib = syscall.NewLazyDLL("user32.dll")
		var proc = userLib.NewProc("EnumDisplayMonitors")

		var monitors []*Monitor

		monitorEnumProc := func(hMonitor int, hdcMonitor int, lprcMonitor unsafe.Pointer, dwData int) int {
			m := (*monitor)(lprcMonitor)
			monitors = append(monitors, &Monitor{m.left, m.top, m.right, m.bottom, m.right - m.left, m.bottom - m.top})
			return 1
		}

		ret, _, err = proc.Call(0,
			0,
			syscall.NewCallback(monitorEnumProc),
			0)
		if ret == 0 {
			return nil, err
		}

		return monitors, nil
	*/

	var monitors []*Monitor

	err := ole.CoInitialize(0)
	if err != nil {
		return nil, err
	}
	defer ole.CoUninitialize()

	desktop, err := ole.CreateInstance(
		ole.NewGUID(CLSID),
		ole.NewGUID(IID))
	if err != nil {
		return nil, err
	}
	defer desktop.Release()

	vtable := (*IDesktopWallpaperVtbl)(unsafe.Pointer(desktop.RawVTable))

	var count uint32

	hr, _, err := syscall.Syscall(
		vtable.GetMonitorDevicePathCount,
		2,
		uintptr(unsafe.Pointer(desktop)),
		uintptr(unsafe.Pointer(&count)),
		0)
	if hr != 0 {
		return nil, fmt.Errorf(
			"Unexpected value from GetMonitorDevicePathCount %d %v", hr, err)
	}

	for i := uint32(0); i < count; i++ {
		var pathOut *[1 << 30]uint16

		hr, _, err = syscall.Syscall(
			vtable.GetMonitorDevicePathAt,
			3,
			uintptr(unsafe.Pointer(desktop)),
			uintptr(i),
			uintptr(unsafe.Pointer(&pathOut)))
		if hr != 0 {
			return nil, fmt.Errorf(
				"Unexpected value from GetMonitorDevicePathAt %d %v", hr, err)
		}

		m := monitor{}
		rectHR, _, errno := syscall.Syscall(
			vtable.GetMonitorRECT,
			3,
			uintptr(unsafe.Pointer(desktop)),
			uintptr(unsafe.Pointer(pathOut)),
			uintptr(unsafe.Pointer(&m)))
		if (rectHR != 0 && rectHR != S_FALSE) || errno != 0 {
			return nil, fmt.Errorf(
				"Unexpected value from GetMonitorRECT %d %v", rectHR, errno)
		}
		// We don't really need to convert to and from []uint16 but this makes
		// debugging easier and allows us to immediately free memory allocated
		// outside of Go's control
		path := syscall.UTF16ToString(pathOut[:])

		_, _, errno = syscall.Syscall(
			coTaskMemFree.Addr(),
			1,
			uintptr(unsafe.Pointer(pathOut)),
			0,
			0)
		if errno != 0 {
			return nil, fmt.Errorf(
				"Unexpected value from CoTaskMemFree %d, %v", hr, err)
		}

		if rectHR == S_FALSE {
			continue
		}

		mon := Monitor{
			// Left:   m.left,
			// Top:    m.top,
			// Right:  m.right,
			// Bottom: m.bottom,
			Width:  int(m.right - m.left),
			Height: int(m.bottom - m.top),
			path:   path}
		monitors = append(monitors, &mon)
	}

	// Prime the aspect ratio caches, completely avoiding the need for locking
	// when syncing the cache since they'll be effectively read only
	for _, m := range monitors {
		m.aspectX, m.aspectY = aspectRatio(m)
	}

	return monitors, nil
}

func SetMonitorWallpapers(monitors []*Monitor) error {
	err := SetRegistryKeys()
	if err != nil {
		return err
	}

	err = ole.CoInitialize(0)
	if err != nil {
		return err
	}
	defer ole.CoUninitialize()

	desktop, err := ole.CreateInstance(
		ole.NewGUID(CLSID),
		ole.NewGUID(IID))
	if err != nil {
		return err
	}
	defer desktop.Release()

	vtable := (*IDesktopWallpaperVtbl)(unsafe.Pointer(desktop.RawVTable))

	hr, _, _ := syscall.Syscall(
		vtable.SetPosition,
		2,
		uintptr(unsafe.Pointer(desktop)),
		DWPOS_CENTER,
		0)
	if hr != 0 {
		return fmt.Errorf("Unexpected value from SetPosition %d", hr)
	}

	for _, m := range monitors {
		hr, _, _ = syscall.Syscall(
			vtable.SetWallpaper,
			3,
			uintptr(unsafe.Pointer(desktop)),
			uintptr(unsafe.Pointer(syscall.StringToUTF16Ptr(m.path))),
			uintptr(unsafe.Pointer(syscall.StringToUTF16Ptr(m.Wallpaper))))
		if hr != 0 {
			return fmt.Errorf("Unexpected value from SetWallpaper %d", hr)
		}
	}

	return nil
}

// TODO -- Uncertain whether this still has any meaning with IDesktopWallpaper
// interfaces
func SetRegistryKeys() error {
	k, err := registry.OpenKey(registry.CURRENT_USER, `Control Panel\Desktop`, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()

	/*err = k.SetStringValue("WallpaperStyle", "0")
	if err != nil {
		return err
	}

	err = k.SetStringValue("TileWallpaper", "1")
	if err != nil {
		return err
	}*/

	err = k.SetDWordValue("JPEGImportQuality", 100)
	return err
}

func checkIfLocked() (bool, error) {
	userLib := syscall.NewLazyDLL("user32.dll")
	openInputDesktop := userLib.NewProc("OpenInputDesktop")
	closeDesktop := userLib.NewProc("CloseDesktop")

	desktop, _, _ := openInputDesktop.Call(0,
		0,
		0)
	if desktop == 0 {
		// Failure here means that the user is on a desktop we cannot access
		// That is overwhelmingly likely to be the lock screen
		return true, nil
	}
	ret, _, _ := closeDesktop.Call(desktop)
	if ret == 0 {
		// If we can open the desktop, not being able to close it is a problem.
		return true, errors.New("Failed to close desktop handle")
	}

	return false, nil
	/*
		Could use this to check the name of a desktop, likely useless

		objectInformation := userLib.NewProc("GetUserObjectInformationW")

		name := [128]uint16{}
		ret, _, err := objectInformation.Call(
			desktop,
			2,
			uintptr(unsafe.Pointer(&name)),
			128)

		log.Println(syscall.UTF16ToString(name[:]))
		if ret == 0 {
			return false, os.NewSyscallError("GetUserObjectInformation", err)
		}*/
}

// Will convert the PNG wallpaper down to a JPEG if it is too large
// TODO -- Remove
/*func ChangeBackground() error {
	c, err := GetConfig()
	if err != nil {
		return err
	}

	if err = SetRegistryKeys(c); err != nil {
		return err
	}

	fi, err := os.Stat(c.WallpaperFile)
	if err != nil {
		return err
	}

	wallpaper := c.WallpaperFile
	if fi.Size() > c.MaxPNGWallpaperSize {
		wallpaper = wallpaper + ".jpg"
		err = ConvertToJPEG(c.WallpaperFile, wallpaper)
		if err != nil {
			return err
		}
	}

	var mod = syscall.NewLazyDLL("user32.dll")
	var proc = mod.NewProc("SystemParametersInfoW")

	ret, _, err := proc.Call(uintptr(0x14),
		0,
		uintptr(unsafe.Pointer(syscall.StringToUTF16Ptr(wallpaper))),
		uintptr(0x3))
	if ret == 0 && err != nil {
		return err
	}

	return nil
}*/

const ATTACH_PARENT_PROCESS = uintptr(^uint32(0)) // (DWORD)-1

var modkernel32 = syscall.NewLazyDLL("kernel32.dll")
var procAttachConsole = modkernel32.NewProc("AttachConsole")

// Attempts to attach to the parent console if one exists so we can get stdout
// Note that it's impossible to properly redirect stdin
// See https://stackoverflow.com/questions/23743217/
func AttachParentConsole() {
	r, _, _ :=
		syscall.Syscall(procAttachConsole.Addr(), 1, ATTACH_PARENT_PROCESS, 0, 0)

	if r == 0 {
		return
	}

	hout, err := syscall.GetStdHandle(syscall.STD_OUTPUT_HANDLE)
	if err != nil {
		return
	}
	herr, err := syscall.GetStdHandle(syscall.STD_ERROR_HANDLE)
	if err != nil {
		return
	}

	os.Stdout = os.NewFile(uintptr(hout), "/dev/stdout")
	os.Stderr = os.NewFile(uintptr(herr), "/dev/stderr")
}
