package importer

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
)

// Importer は、CobraやOS APIから独立した取込フローを組み立てる。
type Importer struct {
	Catalog    ItemCatalog
	Downloader Downloader
	Extractor  ArchiveExtractor
	Store      LibraryStore
}

func (i Importer) Import(ctx context.Context, request ImportRequest) (ImportResult, error) {
	if i.Catalog == nil || i.Downloader == nil || i.Extractor == nil || i.Store == nil {
		return ImportResult{}, errors.New("importer dependencies are incomplete")
	}
	if err := ValidateDownloadURL(request.DownloadURL); err != nil {
		return ImportResult{}, err
	}
	if request.ItemID <= 0 || request.OrderID < 0 || request.VariationID <= 0 || request.DownloadableFilename == "" {
		return ImportResult{}, ErrInvalidImportURI
	}

	metadata, err := i.Catalog.GetItem(ctx, request.ItemID)
	if err != nil {
		return ImportResult{}, fmt.Errorf("get BOOTH item metadata: %w", err)
	}
	if metadata.ItemID != request.ItemID {
		return ImportResult{}, errors.New("BOOTH item metadata ID does not match the import request")
	}

	return i.Store.Replace(ctx, metadata, request.DownloadableFilename, func(workDir, contentDir string) (bool, error) {
		source := filepath.Join(workDir, "download")
		if err := i.Downloader.Download(ctx, request.DownloadURL, source); err != nil {
			return false, err
		}
		extracted, err := i.Extractor.Materialize(ctx, source, request.DownloadableFilename, contentDir)
		if err != nil {
			return false, err
		}
		return extracted, nil
	})
}
