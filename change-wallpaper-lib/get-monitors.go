// +build windows

package changewallpaperlib

import (
	"fmt"
	"syscall"
	"unsafe"

	ole "github.com/go-ole/go-ole"
)

type monitor struct {
	left   int32
	top    int32
	right  int32
	bottom int32
}

type Monitor struct {
	Left   int32
	Top    int32
	Right  int32
	Bottom int32
	Width  int32
	Height int32
	Path   string
}

/*func reflectQueryInterface(self interface{}, method uintptr, interfaceID *GUID, obj interface{}) (err error) {
	objValue := reflect.ValueOf(obj).Elem()

	hr, _, _ := syscall.Syscall(
		method,
		3,
		selfValue.UnsafeAddr(),
		uintptr(unsafe.Pointer(interfaceID)),
		objValue.Addr().Pointer())
	if hr != 0 {
		err = NewError(hr)
	}
	return
}*/

// DesktopWallpaper does not extend
type IDesktopWallpaperVtbl struct {
	QueryInterface            uintptr
	AddRef                    uintptr
	Release                   uintptr
	SetWallpaper              uintptr
	GetWallpaper              uintptr
	GetMonitorDevicePathAt    uintptr
	GetMonitorDevicePathCount uintptr
	GetMonitorRECT            uintptr
}

const CLSID = "{C2CF3110-460E-4fc1-B9D0-8A1C0C9CC4BD}"
const IID = "{B92B56A9-8B55-4E14-9A89-0199BBB6F93B}"

var modole32 = syscall.NewLazyDLL("ole32.dll")
var coTaskMemFree = modole32.NewProc("CoTaskMemFree")

func GetMonitors() ([]*Monitor, error) {
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

	ole.CoInitialize(0)
	defer ole.CoUninitialize()

	desktop, err := ole.CreateInstance(
		ole.NewGUID(CLSID),
		ole.NewGUID(IID))
	if err != nil {
		return nil, err
	}

	vtable := (*IDesktopWallpaperVtbl)(unsafe.Pointer(desktop.RawVTable))

	var count uint32

	hr, _, _ := syscall.Syscall(
		vtable.GetMonitorDevicePathCount,
		2,
		uintptr(unsafe.Pointer(desktop)),
		uintptr(unsafe.Pointer(&count)),
		0)
	if hr != 0 {
		return nil, fmt.Errorf("Unexpected value from GetMonitorDevicePathCount %d", hr)
	}

	for i := uint32(0); i < count; i++ {
		//m := Monitor{}
		var pathOut uintptr

		hr, _, _ = syscall.Syscall(
			vtable.GetMonitorDevicePathAt,
			3,
			uintptr(unsafe.Pointer(desktop)),
			uintptr(i),
			uintptr(unsafe.Pointer(&pathOut)))
		if hr != 0 {
			return nil, fmt.Errorf("Unexpected value from GetMonitorDevicePathAt %d", hr)
		}

		path := syscall.UTF16ToString((*[1<<30 - 1]uint16)(unsafe.Pointer(pathOut))[:])

		m := monitor{}
		hr, _, _ = syscall.Syscall(
			vtable.GetMonitorRECT,
			3,
			uintptr(unsafe.Pointer(desktop)),
			pathOut,
			uintptr(unsafe.Pointer(&m)))
		if hr != 0 || err != nil {
			return nil, fmt.Errorf("Unexpected value from GetMonitorRECT %d %v", hr, err)
		}

		// I think this is right
		hr, _, errno := syscall.Syscall(
			coTaskMemFree.Addr(),
			1,
			pathOut,
			0,
			0)
		if errno != 0 {
			return nil, fmt.Errorf("Unexpected value from CoTaskMemFree %d, %v", hr, err)
		}

		monitors = append(monitors, &Monitor{m.left, m.top, m.right, m.bottom, m.right - m.left, m.bottom - m.top, path})
	}

	return monitors, nil
}
