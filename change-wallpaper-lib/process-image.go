package changewallpaperlib

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	_ "golang.org/x/image/bmp"
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
	//"strings"
	"syscall"
	"time"
	"unicode"
)

// The two models to try from waifu2x caffe, the first one is considered better but can easily run out of memory
const model = `models\upconv_7_anime_style_art_rgb`
const backupModel = `models\anime_style_art_rgb`

// Default mode is to completely fill the target width and height and crop, touch=true only scales the image to touch the insides of the rectangle
// Set denoise=false when the image has already been denoised. Don't double the same image multiple times, instead call this once for each necessary scaling factor
func ProcessImage(inFile, outFile string, width, height int32, touch, denoise bool) error {
	c, err := GetConfig()
	if err != nil {
		return err
	}

	validInFile, img, err := getImageConfig(inFile, c.TempDirectory)
	if err != nil {
		return err
	}

	// Waifu2x only multiplies by powers of 2 and does not downscale so round up to the nearest non-negative power of 2
	scale := getScalingFactor(width, height, touch, img)

	// Assume unknown formats are noisy
	//denoise := strings.HasSuffix(strings.ToLower(inFile), "jpg") || strings.HasSuffix(strings.ToLower(inFile), "jpeg") || inFile != validInFile

	if scale > 1 || denoise {
		var tempFile, err = GetScaledIntermediateFile(outFile, scale)
		if err != nil {
			return err
		} // Should not be possible

		err = w2xProcess(validInFile, tempFile, scale, denoise, c.Waifu2x, model)
		if exitErr, ok := err.(*exec.ExitError); ok {
			if status, ok := exitErr.Sys().(syscall.WaitStatus); ok {
				if status.ExitStatus() == 3221225477 {
					log.Printf("Access violation when running waifu2x on file [%s], retrying with backup model\n", validInFile)
					err = w2xProcess(validInFile, tempFile, scale, denoise, c.Waifu2x, backupModel)
				}
			}
		}

		if err != nil {
			return err
		}
		return imResize(tempFile, outFile, width, height, touch, c.ImageMagick)
	} else {
		return imResize(validInFile, outFile, width, height, touch, c.ImageMagick)
	}
}

func GetScalingFactor(inFile string, width, height int32, touch bool) (int, error) {
	c, err := GetConfig()
	if err != nil {
		return 0, err
	}

	_, img, err := getImageConfig(inFile, c.TempDirectory)
	if err != nil {
		return 0, err
	}

	return getScalingFactor(width, height, touch, img), nil
}

func GetScaledIntermediateFile(outFile string, scale int) (string, error) {
	c, err := GetConfig()
	if err != nil {
		return "", err
	}

	return filepath.Join(c.TempDirectory, fmt.Sprintf("%s-%d-intermediate.bmp", filepath.Base(outFile), scale)), nil
}

func getScalingFactor(width, height int32, touch bool, img *image.Config) int {
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
func getImageConfig(inFile, tempDir string) (string, *image.Config, error) {
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
		h := sha256.Sum256([]byte(inFile))

		// Go has rather limited image format support, try to use imagemagick to convert to a known format
		// Don't use BMP because the BMP library is very limited
		convertedFile := filepath.Join(tempDir, hex.EncodeToString(h[:])+"-converted.png")
		cmd := exec.Command(c.ImageMagick, "convert", inFile, convertedFile)
		cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
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

func imResize(inFile, outFile string, width, height int32, touch bool, imageMagick string) error {
	resStr := fmt.Sprintf("%dx%d", width, height)
	modeStr := "^"
	if touch {
		modeStr = ""
	}

	args := []string{"convert", inFile, "-filter", "Lanczos", "-resize", resStr + modeStr, "-gravity", "center", "-crop", resStr + "+0+0", outFile}
	cmd := exec.Command(imageMagick, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Run()
}

func w2xProcess(inFile, outFile string, scale int, denoise bool, w2x string, modelPath string) error {
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
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	err := cmd.Run()
	if err != nil {
		// CUDA will occasionally fail to initialize if the GPU is still busy after the last call. Retry after a short delay.
		time.Sleep(5 * time.Second)
		cmd := exec.Command(w2x, args...)
		cmd.Dir = wd
		cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
		err = cmd.Run()
		if err != nil {
			log.Printf("Failed twice with settings: %v\n", args)
		}
	}
	return err
}

func Cleanup() error {
	c, err := GetConfig()
	if err != nil {
		return err
	}
	args := []string{"/C", "del", "/Q", c.TempDirectory + "\\*.bmp", c.TempDirectory + "\\*.png"}
	cmd := exec.Command("cmd", args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	/*err = cmd.Run()
	  if err != nil { return err }

	  args = []string{"/C", "del", "/Q", c.TempDirectory + "\\*.bmp"}
	  cmd := exec.Command("cmd", args...)
	  cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	  err = cmd.Run()
	  if err != nil { return err }*/
	return cmd.Run()
}

// Convert to jpeg with quality = 100
// Should only be used when the PNG file size is too large for windows
func ConvertToJPEG(inFile, outFile string) error {
	c, err := GetConfig()
	if err != nil {
		return err
	}

	args := []string{"convert", inFile, "-quality", "100", outFile}
	cmd := exec.Command(c.ImageMagick, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Run()
}
