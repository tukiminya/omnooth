package importer

import (
	"bytes"
	"context"
	"errors"
	"log/slog"
	"net/url"
	"strings"
	"testing"
)

type failingDownloader struct {
	err error
}

func (f failingDownloader) Download(context.Context, *url.URL, string) error {
	return f.err
}

func TestImporterLogsFailedStageWithoutDownloadURL(t *testing.T) {
	var output bytes.Buffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	downloadURL, err := url.Parse("https://download.booth.pm/file?signature=request-secret")
	if err != nil {
		t.Fatal(err)
	}
	service := Importer{
		Catalog: fakeCatalog{metadata: ItemMetadata{
			ItemID: 1, ItemName: "Item", ShopID: 2, ShopName: "Shop",
		}},
		Downloader: failingDownloader{err: errors.New("request https://download.booth.pm/file?signature=error-secret failed")},
		Extractor:  fakeExtractor{},
		Store:      fakeStore{root: t.TempDir()},
		Logger:     logger,
	}

	_, err = service.Import(context.Background(), ImportRequest{
		DownloadURL: downloadURL, DownloadableFilename: "asset.zip",
		ItemID: 1, OrderID: 0, VariationID: 3,
	})
	if err == nil {
		t.Fatal("expected download failure")
	}
	logText := output.String()
	if !strings.Contains(logText, "stage=download") || !strings.Contains(logText, "event=stage_failed") {
		t.Fatalf("download failure stage missing from log: %s", logText)
	}
	if strings.Contains(logText, "request-secret") || strings.Contains(logText, "error-secret") {
		t.Fatalf("download URL leaked into log: %s", logText)
	}
}
