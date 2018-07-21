package changewallpaperlib

import (
	"fmt"
	"syscall"
	"unsafe"

	ole "github.com/go-ole/go-ole"
	"golang.org/x/sys/windows/registry"
)

func SetMonitorWallpaper(m *Monitor, wallpaper string) error {
	//c, _ := GetConfig()
	//if err != nil {
	//	return err
	//}
	//_ = SetRegistryKeys(c)

	ole.CoInitialize(0)
	defer ole.CoUninitialize()

	desktop, err := ole.CreateInstance(
		ole.NewGUID(CLSID),
		ole.NewGUID(IID))
	if err != nil {
		return err
	}

	vtable := (*IDesktopWallpaperVtbl)(unsafe.Pointer(desktop.RawVTable))

	hr, _, _ := syscall.Syscall(
		vtable.SetWallpaper,
		3,
		uintptr(unsafe.Pointer(desktop)),
		uintptr(unsafe.Pointer(syscall.StringToUTF16Ptr(m.Path))),
		uintptr(unsafe.Pointer(syscall.StringToUTF16Ptr(wallpaper))))
	if hr != 0 {
		return fmt.Errorf("Unexpected value from SetWallpaper %d", hr)
	}

	return nil
}

// TODO -- This is likely no longer necessary
// Replace with calls to SetPosition
func SetRegistryKeys(c *Config) error {
	k, err := registry.OpenKey(registry.CURRENT_USER, `Control Panel\Desktop`, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()

	err = k.SetStringValue("WallpaperStyle", "0")
	if err != nil {
		return err
	}

	err = k.SetStringValue("TileWallpaper", "1")
	if err != nil {
		return err
	}

	err = k.SetDWordValue("JPEGImportQuality", 100)
	return err
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
