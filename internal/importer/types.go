package importer

import (
	"context"
	"net/url"
)

// ImportRequest は、BOOTH Library Manager URLから検証済みの入力を保持する。
type ImportRequest struct {
	DownloadURL          *url.URL
	DownloadableFilename string
	ItemID               int64
	// OrderID は、ギフト商品の場合に0となる。
	OrderID     int64
	VariationID int64
}

// ItemMetadata は、取込商品の配置に必要なBOOTHの商品情報を保持する。
type ItemMetadata struct {
	ItemID   int64
	ItemName string
	ShopID   int64
	ShopName string
}

// ImportResult は、正常に配置されたダウンロードの結果を表す。
type ImportResult struct {
	Destination string
	Extracted   bool
}

// ItemCatalog は、BOOTHの商品情報を取得する。
type ItemCatalog interface {
	GetItem(context.Context, int64) (ItemMetadata, error)
}

// Downloader は、検証済みURLの内容を保存先へストリーミングする。
type Downloader interface {
	Download(context.Context, *url.URL, string) error
}

// ArchiveExtractor は、対応書庫を展開し、通常ファイルをコピーする。
type ArchiveExtractor interface {
	Materialize(ctx context.Context, sourcePath, filename, destination string) (bool, error)
}

// LibraryStore は、配置先のロック、一時配置、アトミックな置換を担う。
type LibraryStore interface {
	Replace(
		ctx context.Context,
		metadata ItemMetadata,
		downloadableFilename string,
		populate func(workDir, contentDir string) (bool, error),
	) (ImportResult, error)
}
