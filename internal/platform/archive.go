package platform

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"strings"

	"github.com/bodgit/sevenzip"
	"github.com/mholt/archives"
)

var ErrEncryptedArchive = errors.New("encrypted archives are not supported")

type ArchiveMaterializer struct{}

func (ArchiveMaterializer) Materialize(ctx context.Context, sourcePath, filename, destination string) (bool, error) {
	input, err := os.Open(sourcePath)
	if err != nil {
		return false, fmt.Errorf("open download: %w", err)
	}
	format, stream, identifyErr := archives.Identify(ctx, filename, input)
	if identifyErr != nil && !errors.Is(identifyErr, archives.NoMatch) {
		input.Close()
		return false, fmt.Errorf("identify archive: %w", identifyErr)
	}
	extractor, isArchive := format.(archives.Extractor)
	if errors.Is(identifyErr, archives.NoMatch) || !isArchive {
		if err := input.Close(); err != nil {
			return false, fmt.Errorf("close download: %w", err)
		}
		target := filepath.Join(destination, SanitizeComponent(filename))
		if err := os.Rename(sourcePath, target); err != nil {
			return false, fmt.Errorf("place downloaded file: %w", err)
		}
		if err := os.Chmod(target, 0o644); err != nil {
			return false, fmt.Errorf("set downloaded file permissions: %w", err)
		}
		return false, nil
	}

	extractErr := extractor.Extract(ctx, stream, func(ctx context.Context, file archives.FileInfo) error {
		return extractEntry(ctx, destination, file)
	})
	closeErr := input.Close()
	if extractErr != nil {
		if looksEncrypted(extractErr) {
			return false, ErrEncryptedArchive
		}
		return false, fmt.Errorf("extract archive: %w", extractErr)
	}
	if closeErr != nil {
		return false, fmt.Errorf("close archive: %w", closeErr)
	}
	if err := os.Remove(sourcePath); err != nil {
		return false, fmt.Errorf("remove extracted archive: %w", err)
	}
	return true, nil
}

func extractEntry(ctx context.Context, destination string, file archives.FileInfo) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	relative, err := safeArchivePath(file.NameInArchive)
	if err != nil {
		return err
	}
	if relative == "." {
		return nil
	}
	if file.LinkTarget != "" || file.Mode()&fs.ModeSymlink != 0 {
		return fmt.Errorf("archive links are not allowed: %s", file.NameInArchive)
	}
	modeType := file.Mode() & fs.ModeType
	target := filepath.Join(destination, filepath.FromSlash(relative))
	if file.IsDir() {
		if modeType != fs.ModeDir {
			return fmt.Errorf("unsupported archive entry type: %s", file.NameInArchive)
		}
		return os.MkdirAll(target, 0o755)
	}
	if modeType != 0 {
		return fmt.Errorf("unsupported archive entry type: %s", file.NameInArchive)
	}
	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		return fmt.Errorf("create archive directory: %w", err)
	}
	reader, err := file.Open()
	if err != nil {
		return fmt.Errorf("open archive entry %s: %w", file.NameInArchive, err)
	}
	defer reader.Close()
	permissions := fs.FileMode(0o644) | (file.Mode().Perm() & 0o111)
	output, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, permissions)
	if err != nil {
		return fmt.Errorf("create archive entry %s: %w", file.NameInArchive, err)
	}
	_, copyErr := copyWithContext(ctx, output, reader)
	closeErr := output.Close()
	if copyErr != nil {
		return fmt.Errorf("write archive entry %s: %w", file.NameInArchive, copyErr)
	}
	if closeErr != nil {
		return fmt.Errorf("close archive entry %s: %w", file.NameInArchive, closeErr)
	}
	return nil
}

func safeArchivePath(name string) (string, error) {
	if name == "" || strings.ContainsRune(name, '\x00') {
		return "", errors.New("archive contains an invalid empty path")
	}
	normalized := strings.ReplaceAll(name, "\\", "/")
	if strings.HasPrefix(normalized, "/") || path.IsAbs(normalized) {
		return "", fmt.Errorf("archive contains an absolute path: %s", name)
	}
	cleaned := path.Clean(normalized)
	if cleaned == ".." || strings.HasPrefix(cleaned, "../") {
		return "", fmt.Errorf("archive path escapes the destination: %s", name)
	}
	for _, component := range strings.Split(cleaned, "/") {
		if component == "" || component == "." {
			continue
		}
		if strings.Contains(component, ":") {
			return "", fmt.Errorf("archive contains a Windows volume or ADS path: %s", name)
		}
	}
	return cleaned, nil
}

func looksEncrypted(err error) bool {
	var sevenZipError *sevenzip.ReadError
	if errors.As(err, &sevenZipError) && sevenZipError.Encrypted {
		return true
	}
	message := strings.ToLower(err.Error())
	return strings.Contains(message, "password") || strings.Contains(message, "encrypted") || strings.Contains(message, "incorrect key")
}
