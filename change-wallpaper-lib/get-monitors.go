package changewallpaperlib

import (
    "syscall"
    "unsafe"
)

type monitor struct {
    left int32
    top int32
    right int32
    bottom int32
}

type Monitor struct {
    Left int32
    Top int32
    Right int32
    Bottom int32
    Width int32
    Height int32
}

func GetMonitors() ([]*Monitor, error) {
    var dpiLib = syscall.NewLazyDLL("shcore.dll")
    var dpiProc = dpiLib.NewProc("SetProcessDpiAwareness")

    ret, _, err := dpiProc.Call(uintptr(2))
    if ret != 0 {
        return nil, err
    }

    var userLib = syscall.NewLazyDLL("user32.dll")
    var proc = userLib.NewProc("EnumDisplayMonitors")

    var monitors []*Monitor

    monitorEnumProc := func (hMonitor int, hdcMonitor int, lprcMonitor unsafe.Pointer, dwData int) int {
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
}
