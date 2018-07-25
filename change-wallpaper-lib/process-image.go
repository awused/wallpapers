package changewallpaperlib

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	_ "image/png"
	"log"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"sync"

	_ "golang.org/x/image/bmp"
	//"strings"
	"syscall"
	"time"
	"unicode"
)

// The two models to try from waifu2x caffe, the first one is considered better but can easily run out of memory
const model = `models\upconv_7_anime_style_art_rgb`
const backupModel = `models\anime_style_art_rgb`

var gpuLock sync.Mutex

// Offset arguments for cropping, expressed as positive or negative percentages
// of the image. Setting both at once is a mistake, right now.
// TODO -- Support more flexible cropping
type CropOffset struct {
	Vertical   float64 // Note that +vertical is up, not down
	Horizontal float64
}

type ProcessOptions struct {
	Input  AbsolutePath
	Output AbsolutePath
	Width  int
	Height int
	// Default is "Fill", set touch to true to only touch the insides of the
	// target rectangle
	Touch bool
	// Denoise the image. Recommended to always be true as many anime images,
	// even pngs, contain undesired noise
	Denoise bool
	// Flatten transparency. Generally good to set this to tue for wallpapers
	// TODO -- Add a configuration option for background colour
	Flatten bool
	Offset  CropOffset
}

func validateProcessOptions(po ProcessOptions) bool {
	return !(po.Input == "" || po.Output == "" || po.Width == 0 || po.Height == 0)
}

// Default mode is to completely fill the target width and height and crop, touch=true only scales the image to touch the insides of the rectangle
// Set denoise=false when the image has already been denoised. Don't double the same image multiple times, instead call this once for each necessary scaling factor
func ProcessImage(po ProcessOptions) error {
	if !validateProcessOptions(po) {
		return fmt.Errorf("ProcessOptions missing required field")
	}

	c, err := GetConfig()
	if err != nil {
		return err
	}

	validInFile, img, err := getImageConfig(po.Input)
	if err != nil {
		return err
	}

	err = createMissingDirectories(po.Output)
	if err != nil {
		return err
	}

	// Waifu2x only multiplies by powers of 2 and does not downscale so round up to the nearest non-negative power of 2
	scale := getScalingFactor(po.Width, po.Height, po.Touch, img)

	// Assume unknown formats are noisy
	//denoise := strings.HasSuffix(strings.ToLower(inFile), "jpg") || strings.HasSuffix(strings.ToLower(inFile), "jpeg") || inFile != validInFile

	if scale > 1 || po.Denoise {
		var tempFile, err = GetScaledIntermediateFile(po.Output, scale)
		if err != nil {
			return err
		} // Should not be possible

		gpuLock.Lock()
		err = w2xProcess(validInFile, tempFile, scale, po.Denoise, c.Waifu2x, model)
		if exitErr, ok := err.(*exec.ExitError); ok {
			if status, ok := exitErr.Sys().(syscall.WaitStatus); ok {
				if status.ExitStatus() == 3221225477 {
					log.Printf("Access violation when running waifu2x on file [%s], retrying with backup model\n", validInFile)
					err = w2xProcess(validInFile, tempFile, scale, po.Denoise, c.Waifu2x, backupModel)
				}
			}
		}
		gpuLock.Unlock()

		if err != nil {
			return err
		}
		return imResize(tempFile, po.Output, po, img, c.ImageMagick)
	} else {
		return imResize(validInFile, po.Output, po, img, c.ImageMagick)
	}
}

func GetScalingFactor(inFile AbsolutePath, width, height int, touch bool) (int, error) {
	_, img, err := getImageConfig(inFile)
	if err != nil {
		return 0, err
	}

	return getScalingFactor(width, height, touch, img), nil
}

func GetScaledIntermediateFile(outFile AbsolutePath, scale int) (string, error) {
	tdir, err := TempDir()
	if err != nil {
		return "", err
	}

	f := filepath.Join(tdir, hashPath(outFile)) + "-" + strconv.Itoa(scale) + "-intermediate.bmp"

	return f, nil
}

func getScalingFactor(width, height int, touch bool, img *image.Config) int {
	if touch {
		return int(math.Pow(2,
			math.Max(
				math.Ceil(math.Log2(math.Min(float64(width)/float64(img.Width), float64(height)/float64(img.Height)))),
				0)))
	} else {
		return int(math.Pow(2,
			math.Max(
				math.Ceil(math.Log2(math.Max(float64(width)/float64(img.Width), float64(height)/float64(img.Height)))),
				0)))
	}
}

// Gets the image.Config of the input image file, converting to bitmap if necessary
func getImageConfig(inFile AbsolutePath) (string, *image.Config, error) {
	c, err := GetConfig()
	if err != nil {
		return "", nil, err
	}

	in, err := os.Open(inFile)
	if err != nil {
		return "", nil, err
	}
	defer in.Close()

	fi, err := in.Stat()
	if err != nil {
		return "", nil, err
	}

	if !fi.Mode().IsRegular() {
		return "", nil, fmt.Errorf("Input image [%s] is not a regular file", inFile)
	}

	// Waifu2x CLI interface doesn't accept unicode from Go, treat unicode input as if it needs to be converted
	shouldRename := false
	for _, chr := range inFile {
		if chr > unicode.MaxASCII {
			shouldRename = true
			break
		}
	}

	img, _, err := image.DecodeConfig(in)
	if err != nil && err.Error() != "image: unknown format" && err.Error() != "bmp: unsupported BMP image" {
		return "", nil, err
	}
	if err != nil || shouldRename {
		tdir, err := TempDir()
		if err != nil {
			return "", nil, err
		}

		// Go has rather limited image format support, try to use imagemagick to convert to a known format
		// Don't use BMP because the BMP library is very limited
		convertedFile := filepath.Join(tdir, hashPath(inFile)+"-converted.png")
		cmd := exec.Command(c.ImageMagick, "convert", inFile, convertedFile)
		cmd.SysProcAttr = sysProcAttr
		err = cmd.Run()
		if err != nil {
			return "", nil, err
		}

		inc, err := os.Open(convertedFile)
		if err != nil {
			return "", nil, err
		}
		defer inc.Close()

		fi, err = in.Stat()
		if err != nil {
			return "", nil, err
		}

		if !fi.Mode().IsRegular() {
			return "", nil, fmt.Errorf("Input image [%s] is not a regular file", inFile)
		}

		img, _, err = image.DecodeConfig(inc)
		if err != nil {
			return "", nil, err
		}
		return convertedFile, &img, nil
	}

	return inFile, &img, nil
}

func imResize(inFile, outFile AbsolutePath, po ProcessOptions, img *image.Config, imageMagick string) error {
	resStr := fmt.Sprintf("%dx%d", po.Width, po.Height)
	modeStr := "^"
	if po.Touch {
		modeStr = ""
	}

	offsetString := ""

	if po.Offset.Horizontal != 0 || po.Offset.Vertical != 0 {
		// Choose the right offset scaling factor regardless of whether the image
		// is taller or wider than the monitor
		offsetScale := math.Max(
			float64(po.Width)/float64(img.Width),
			float64(po.Height)/float64(img.Height))

		// Multiple voff by negative 1 so that it's more intuitive in configs
		// Also Most of my offsets would be negative if I didn't
		voff := -1 * offsetScale * po.Offset.Vertical * float64(img.Height) / 100
		hoff := offsetScale * po.Offset.Horizontal * float64(img.Width) / 100

		offsetString = fmt.Sprintf("%+d%+d!", int(hoff), int(voff))
	}

	args := []string{
		"convert", inFile,
		"-filter", "Lanczos",
		"-resize", resStr + modeStr,
		"-gravity", "center",
		"-crop", resStr + offsetString}

	if po.Flatten {
		// Transparency appears to break when using waifu2x-caffe, so flatten
		// May be able to revisit in the future, but for now just flatten
		// This is very CPU intensive
		args = append(args, "-flatten")
	}

	args = append(args, outFile)

	cmd := exec.Command(imageMagick, args...)
	cmd.SysProcAttr = sysProcAttr
	return cmd.Run()
}

func w2xProcess(inFile, outFile AbsolutePath, scale int, denoise bool, w2x string, modelPath string) error {
	if scale < 1 {
		return fmt.Errorf("Cannot use waifu2x with a scale less than 1")
	}
	wd := filepath.Dir(w2x)
	mode := "noise_scale"
	if scale == 1 {
		mode = "noise"
	} else if !denoise {
		mode = "scale"
	}

	args := []string{"-m", mode, "-i", inFile, "-o", outFile, "--model_dir", modelPath}
	if scale != 1 {
		args = append(args, "-s", strconv.Itoa(scale))
	}

	if denoise {
		args = append(args, "-n", "1")
	}

	cmd := exec.Command(w2x, args...)
	cmd.Dir = wd
	cmd.SysProcAttr = sysProcAttr
	err := cmd.Run()
	if err != nil {
		// CUDA will occasionally fail to initialize if the GPU is still busy after the last call. Retry after a short delay.
		time.Sleep(5 * time.Second)
		cmd := exec.Command(w2x, args...)
		cmd.Dir = wd
		cmd.SysProcAttr = sysProcAttr
		err = cmd.Run()
		if err != nil {
			log.Printf("Failed twice with settings: %v\n", args)
		}
	}
	return err
}

// Used to avoid collisions when creating temporary files
// It's either this or created deep nested directories inside TempDir
// Which _might_ be a better option
func hashPath(path AbsolutePath) string {
	h := sha256.Sum256([]byte(path))
	return hex.EncodeToString(h[:])
}

func createMissingDirectories(outFile AbsolutePath) error {
	return os.MkdirAll(filepath.Dir(outFile), 0755)
}

// Convert to jpeg with quality = 100
// Should only be used when the PNG file size is too large for windows
// TODO -- Cleanup
/*func ConvertToJPEG(inFile, outFile string) error {
	c, err := GetConfig()
	if err != nil {
		return err
	}

	args := []string{"convert", inFile, "-quality", "100", outFile}
	cmd := exec.Command(c.ImageMagick, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Run()
}*/
