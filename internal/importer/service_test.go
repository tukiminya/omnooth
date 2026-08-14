package importer

import (
	"context"
	"errors"
	"net/url"
	"os"
	"path/filepath"
	"testing"
)

type fakeCatalog struct {
	metadata ItemMetadata
	err      error
}

func (f fakeCatalog) GetItem(context.Context, int64) (ItemMetadata, error) {
	return f.metadata, f.err
}

type fakeDownloader struct{}

func (fakeDownloader) Download(_ context.Context, _ *url.URL, destination string) error {
	return os.WriteFile(destination, []byte("download"), 0o600)
}

type fakeExtractor struct{}

func (fakeExtractor) Materialize(_ context.Context, source, _ string, destination string) (bool, error) {
	content, err := os.ReadFile(source)
	if err != nil {
		return false, err
	}
	return true, os.WriteFile(filepath.Join(destination, "asset.txt"), content, 0o644)
}

type fakeStore struct {
	root string
}

func (f fakeStore) Replace(
	ctx context.Context,
	_ ItemMetadata,
	_ string,
	populate func(string, string) (bool, error),
) (ImportResult, error) {
	work := filepath.Join(f.root, "work")
	content := filepath.Join(f.root, "content")
	if err := os.MkdirAll(work, 0o755); err != nil {
		return ImportResult{}, err
	}
	if err := os.MkdirAll(content, 0o755); err != nil {
		return ImportResult{}, err
	}
	extracted, err := populate(work, content)
	return ImportResult{Destination: content, Extracted: extracted}, err
}

func TestImporterImport(t *testing.T) {
	downloadURL, _ := url.Parse("https://download.booth.pm/file")
	service := Importer{
		Catalog:    fakeCatalog{metadata: ItemMetadata{ItemID: 1, ItemName: "Item", ShopID: 2, ShopName: "Shop"}},
		Downloader: fakeDownloader{},
		Extractor:  fakeExtractor{},
		Store:      fakeStore{root: t.TempDir()},
	}
	result, err := service.Import(context.Background(), ImportRequest{
		DownloadURL: downloadURL, DownloadableFilename: "asset.zip",
		ItemID: 1, OrderID: 2, VariationID: 3,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !result.Extracted {
		t.Fatal("expected extracted result")
	}
	content, err := os.ReadFile(filepath.Join(result.Destination, "asset.txt"))
	if err != nil || string(content) != "download" {
		t.Fatalf("materialized content = %q, err = %v", content, err)
	}
}

func TestImporterRejectsMetadataMismatch(t *testing.T) {
	downloadURL, _ := url.Parse("https://booth.pm/file")
	service := Importer{
		Catalog:    fakeCatalog{metadata: ItemMetadata{ItemID: 99, ItemName: "Item", ShopID: 2, ShopName: "Shop"}},
		Downloader: fakeDownloader{}, Extractor: fakeExtractor{}, Store: fakeStore{root: t.TempDir()},
	}
	_, err := service.Import(context.Background(), ImportRequest{
		DownloadURL: downloadURL, DownloadableFilename: "asset.zip", ItemID: 1, OrderID: 2, VariationID: 3,
	})
	if err == nil || errors.Is(err, ErrInvalidImportURI) {
		t.Fatalf("expected metadata mismatch, got %v", err)
	}
}
