package changewallpaperlib

import (
    "golang.org/x/sys/windows/registry"
    "syscall"
    "unsafe"
    "os"
)

// Will convert the PNG wallpaper down to a JPEG if it is too large
func ChangeBackground() error {
    c, err := GetConfig()
    if err != nil { return err }

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
    if err != nil {
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
}
