package platform

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/tukiminya/omnooth/internal/importer"
)

var testMetadata = importer.ItemMetadata{ItemID: 8657397, ItemName: "Item/Name", ShopID: 838775, ShopName: "Shop:Name"}

func TestSanitizeComponent(t *testing.T) {
	tests := map[string]string{
		"日本語 name.zip":  "日本語 name.zip",
		"../evil\\file": "_evil_file",
		"CON":           "_CON",
		"con.txt":       "_con.txt",
		"a\x00b":        "a_b",
		"...":           "_",
	}
	for input, expected := range tests {
		if actual := SanitizeComponent(input); actual != expected {
			t.Errorf("SanitizeComponent(%q) = %q, want %q", input, actual, expected)
		}
	}
}

func TestLocalStoreReplacesWholeDownloadDirectory(t *testing.T) {
	root := t.TempDir()
	store := NewLocalStore(root)
	first, err := store.Replace(context.Background(), testMetadata, "asset.zip", testInstallation(), func(_, content string) (bool, error) {
		if err := os.WriteFile(filepath.Join(content, "old.txt"), []byte("old"), 0o644); err != nil {
			return false, err
		}
		return true, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(first.Destination, filepath.Join("838775_Shop_Name", "8657397_Item_Name", "asset.zip")) {
		t.Fatalf("destination = %s", first.Destination)
	}
	second, err := store.Replace(context.Background(), testMetadata, "asset.zip", testInstallation(), func(_, content string) (bool, error) {
		return false, os.WriteFile(filepath.Join(content, "new.txt"), []byte("new"), 0o644)
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(second.Destination, "old.txt")); !os.IsNotExist(err) {
		t.Fatalf("old file remains: %v", err)
	}
	if content, err := os.ReadFile(filepath.Join(second.Destination, "new.txt")); err != nil || string(content) != "new" {
		t.Fatalf("new file = %q, err = %v", content, err)
	}
}

func TestLocalStorePreservesPreviousVersionOnFailure(t *testing.T) {
	store := NewLocalStore(t.TempDir())
	result, err := store.Replace(context.Background(), testMetadata, "asset.zip", testInstallation(), func(_, content string) (bool, error) {
		return true, os.WriteFile(filepath.Join(content, "old.txt"), []byte("old"), 0o644)
	})
	if err != nil {
		t.Fatal(err)
	}
	wantErr := errors.New("extract failed")
	_, err = store.Replace(context.Background(), testMetadata, "asset.zip", testInstallation(), func(_, content string) (bool, error) {
		_ = os.WriteFile(filepath.Join(content, "partial.txt"), []byte("partial"), 0o644)
		return false, wantErr
	})
	if !errors.Is(err, wantErr) {
		t.Fatalf("error = %v", err)
	}
	if content, err := os.ReadFile(filepath.Join(result.Destination, "old.txt")); err != nil || string(content) != "old" {
		t.Fatalf("old file = %q, err = %v", content, err)
	}
	if _, err := os.Stat(filepath.Join(result.Destination, "partial.txt")); !os.IsNotExist(err) {
		t.Fatalf("partial file was committed: %v", err)
	}
}

func TestLocalStoreSerializesSameTarget(t *testing.T) {
	store := NewLocalStore(t.TempDir())
	entered := make(chan struct{})
	release := make(chan struct{})
	firstDone := make(chan error, 1)
	go func() {
		_, err := store.Replace(context.Background(), testMetadata, "asset.zip", testInstallation(), func(_, _ string) (bool, error) {
			close(entered)
			<-release
			return false, nil
		})
		firstDone <- err
	}()
	<-entered
	ctx, cancel := context.WithTimeout(context.Background(), 150*time.Millisecond)
	defer cancel()
	_, secondErr := store.Replace(ctx, testMetadata, "asset.zip", testInstallation(), func(_, _ string) (bool, error) {
		t.Fatal("second populate ran while target was locked")
		return false, nil
	})
	if !errors.Is(secondErr, context.DeadlineExceeded) {
		t.Fatalf("second error = %v", secondErr)
	}
	close(release)
	if err := <-firstDone; err != nil {
		t.Fatal(err)
	}
}

func TestLocalStoreWritesManifestAndRejectsReservedPath(t *testing.T) {
	store := NewLocalStore(t.TempDir())
	result, err := store.Replace(context.Background(), testMetadata, "asset.zip", testInstallation(), func(_, content string) (bool, error) {
		return false, os.WriteFile(filepath.Join(content, "asset.txt"), []byte("asset"), 0o644)
	})
	if err != nil {
		t.Fatal(err)
	}
	manifest, err := os.ReadFile(filepath.Join(result.Destination, ".omnooth", "installation.json"))
	if err != nil || !strings.Contains(string(manifest), `"schema_version": 1`) {
		t.Fatalf("manifest = %q, err = %v", manifest, err)
	}

	_, err = store.Replace(context.Background(), testMetadata, "other.zip", func() importer.InstallationMetadata {
		value := testInstallation()
		value.DownloadableFilename = "other.zip"
		return value
	}(), func(_, content string) (bool, error) {
		return false, os.Mkdir(filepath.Join(content, ".omnooth"), 0o755)
	})
	if err == nil || !strings.Contains(err.Error(), "reserved .omnooth") {
		t.Fatalf("error = %v", err)
	}
}

func testInstallation() importer.InstallationMetadata {
	return importer.InstallationMetadata{
		SchemaVersion: importer.InstallationSchemaVersion,
		Item:          testMetadata, VariationID: 3, DownloadableFilename: "asset.zip",
		InstalledAt: time.Now().UTC(),
	}
}
