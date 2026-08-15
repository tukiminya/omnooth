package importer

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"path/filepath"
	"time"

	"github.com/tukiminya/omnooth/internal/runlog"
)

// Importer は、CobraやOS APIから独立した取込フローを組み立てる。
type Importer struct {
	Catalog    ItemCatalog
	Downloader Downloader
	Extractor  ArchiveExtractor
	Store      LibraryStore
	Logger     *slog.Logger
}

func (i Importer) Import(ctx context.Context, request ImportRequest) (ImportResult, error) {
	logger := i.Logger
	if logger == nil {
		logger = slog.New(slog.DiscardHandler)
	}
	started := time.Now()
	logger.Info("import started",
		"event", "import_started",
		"item_id", request.ItemID,
		"variation_id", request.VariationID,
		"downloadable_filename", request.DownloadableFilename,
	)
	if i.Catalog == nil || i.Downloader == nil || i.Extractor == nil || i.Store == nil {
		err := errors.New("importer dependencies are incomplete")
		logger.Error("import failed", append([]any{"event", "import_failed", "stage", "validate_dependencies"}, runlog.ErrorAttrs(err)...)...)
		return ImportResult{}, err
	}
	if err := ValidateDownloadURL(request.DownloadURL); err != nil {
		logger.Error("import failed", append([]any{"event", "import_failed", "stage", "validate_request"}, runlog.ErrorAttrs(err)...)...)
		return ImportResult{}, err
	}
	if request.ItemID <= 0 || request.OrderID < 0 || request.VariationID <= 0 || request.DownloadableFilename == "" {
		logger.Error("import failed", append([]any{"event", "import_failed", "stage", "validate_request"}, runlog.ErrorAttrs(ErrInvalidImportURI)...)...)
		return ImportResult{}, ErrInvalidImportURI
	}

	stageStarted := time.Now()
	logger.Info("stage started", "event", "stage_started", "stage", "get_item_metadata")
	metadata, err := i.Catalog.GetItem(ctx, request.ItemID)
	if err != nil {
		err = fmt.Errorf("get BOOTH item metadata: %w", err)
		logger.Error("stage failed", append([]any{"event", "stage_failed", "stage", "get_item_metadata", "duration", time.Since(stageStarted)}, runlog.ErrorAttrs(err)...)...)
		return ImportResult{}, err
	}
	logger.Info("stage completed", "event", "stage_completed", "stage", "get_item_metadata", "duration", time.Since(stageStarted))
	if metadata.ItemID != request.ItemID {
		err := errors.New("BOOTH item metadata ID does not match the import request")
		logger.Error("import failed", append([]any{"event", "import_failed", "stage", "validate_metadata"}, runlog.ErrorAttrs(err)...)...)
		return ImportResult{}, err
	}

	stageStarted = time.Now()
	logger.Info("stage started", "event", "stage_started", "stage", "store_import")
	result, err := i.Store.Replace(ctx, metadata, request.DownloadableFilename, func(workDir, contentDir string) (bool, error) {
		source := filepath.Join(workDir, "download")
		downloadStarted := time.Now()
		logger.Info("stage started", "event", "stage_started", "stage", "download")
		if err := i.Downloader.Download(ctx, request.DownloadURL, source); err != nil {
			err = fmt.Errorf("download file: %w", err)
			logger.Error("stage failed", append([]any{"event", "stage_failed", "stage", "download", "duration", time.Since(downloadStarted)}, runlog.ErrorAttrs(err)...)...)
			return false, err
		}
		logger.Info("stage completed", "event", "stage_completed", "stage", "download", "duration", time.Since(downloadStarted))

		materializeStarted := time.Now()
		logger.Info("stage started", "event", "stage_started", "stage", "materialize")
		extracted, err := i.Extractor.Materialize(ctx, source, request.DownloadableFilename, contentDir)
		if err != nil {
			err = fmt.Errorf("materialize download: %w", err)
			logger.Error("stage failed", append([]any{"event", "stage_failed", "stage", "materialize", "duration", time.Since(materializeStarted)}, runlog.ErrorAttrs(err)...)...)
			return false, err
		}
		logger.Info("stage completed", "event", "stage_completed", "stage", "materialize", "duration", time.Since(materializeStarted), "extracted", extracted)
		return extracted, nil
	})
	if err != nil {
		err = fmt.Errorf("store import: %w", err)
		logger.Error("stage failed", append([]any{"event", "stage_failed", "stage", "store_import", "duration", time.Since(stageStarted)}, runlog.ErrorAttrs(err)...)...)
		logger.Error("import failed", "event", "import_failed", "stage", "store_import", "duration", time.Since(started))
		return ImportResult{}, err
	}
	logger.Info("stage completed", "event", "stage_completed", "stage", "store_import", "duration", time.Since(stageStarted))
	logger.Info("import completed", "event", "import_completed", "duration", time.Since(started), "destination", result.Destination, "extracted", result.Extracted)
	return result, nil
}
