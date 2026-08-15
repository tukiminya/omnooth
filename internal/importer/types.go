package importer

import (
	"context"
	"net/url"
	"time"
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
	ItemID     int64               `json:"item_id"`
	ItemName   string              `json:"item_name"`
	ShopID     int64               `json:"shop_id"`
	ShopName   string              `json:"shop_name"`
	Variations []VariationMetadata `json:"variations,omitempty"`
}

type VariationMetadata struct {
	ID            int64                  `json:"id"`
	Name          string                 `json:"name,omitempty"`
	Type          string                 `json:"type"`
	Downloadables []DownloadableMetadata `json:"downloadables"`
}

type DownloadableMetadata struct {
	Name      string    `json:"name"`
	FileSize  string    `json:"file_size"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

const InstallationSchemaVersion = 1

// InstallationMetadata is stored beside imported content so future catalog
// responses can be compared with the state that was visible during import.
type InstallationMetadata struct {
	SchemaVersion        int                `json:"schema_version"`
	Item                 ItemMetadata       `json:"item"`
	VariationID          int64              `json:"variation_id"`
	VariationName        string             `json:"variation_name,omitempty"`
	DownloadableFilename string             `json:"downloadable_filename"`
	InstalledAt          time.Time          `json:"installed_at"`
	Snapshot             *VariationMetadata `json:"snapshot,omitempty"`
	TrackingIssue        string             `json:"tracking_issue,omitempty"`
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
		installation InstallationMetadata,
		populate func(workDir, contentDir string) (bool, error),
	) (ImportResult, error)
}
