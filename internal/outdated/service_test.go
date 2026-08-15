package outdated

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/tukiminya/omnooth/internal/importer"
)

type catalogStub struct {
	items map[int64]importer.ItemMetadata
	err   error
	calls int
}

func (c *catalogStub) GetItem(_ context.Context, itemID int64) (importer.ItemMetadata, error) {
	c.calls++
	if c.err != nil {
		return importer.ItemMetadata{}, c.err
	}
	return c.items[itemID], nil
}

func TestCompareDownloadableChanges(t *testing.T) {
	base := downloadable("asset.zip", "1 MB", 1)
	installed := variation(3, base)
	tests := []struct {
		name    string
		current importer.VariationMetadata
		want    string
	}{
		{"unchanged", variation(3, base), ""},
		{"display order is irrelevant", variation(3, base), ""},
		{"added", variation(3, base, downloadable("new.zip", "2 MB", 2)), "added: new.zip"},
		{"removed", variation(3), "removed: asset.zip"},
		{"modified", variation(3, downloadable("asset.zip", "2 MB", 1)), "modified: asset.zip"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			changes := compare(installed, test.current)
			if test.want == "" && len(changes) != 0 {
				t.Fatalf("changes = %v", changes)
			}
			if test.want != "" && (len(changes) != 1 || changes[0] != test.want) {
				t.Fatalf("changes = %v, want %q", changes, test.want)
			}
		})
	}
}

func TestServiceGroupsVariationAndOnlyListsStaleInstallations(t *testing.T) {
	root := t.TempDir()
	old := variation(3, downloadable("v1.zip", "1 MB", 1))
	current := variation(3, downloadable("v1.zip", "1 MB", 1), downloadable("v2.zip", "2 MB", 2))
	writeInstallation(t, root, "v1.zip", old)
	writeInstallation(t, root, "v2.zip", current)
	catalog := &catalogStub{items: map[int64]importer.ItemMetadata{1: {
		ItemID: 1, ItemName: "Item", ShopID: 2, ShopName: "Shop", Variations: []importer.VariationMetadata{current},
	}}}
	result, err := (Service{Catalog: catalog, Root: root}).Check(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if catalog.calls != 1 {
		t.Fatalf("catalog calls = %d", catalog.calls)
	}
	if len(result.Reports) != 1 {
		t.Fatalf("reports = %+v", result.Reports)
	}
	report := result.Reports[0]
	if report.Status != StatusOutdated || len(report.LocalDownloadables) != 1 || report.LocalDownloadables[0] != "v1.zip" {
		t.Fatalf("report = %+v", report)
	}
	if len(report.Changes) != 1 || report.Changes[0] != "added: v2.zip" {
		t.Fatalf("changes = %v", report.Changes)
	}
}

func TestServiceReportsLegacyAndCatalogFailureAsUnknown(t *testing.T) {
	root := t.TempDir()
	legacy := filepath.Join(root, "items", "2_Shop", "1_Item", "legacy.zip")
	if err := os.MkdirAll(legacy, 0o755); err != nil {
		t.Fatal(err)
	}
	catalog := &catalogStub{err: errors.New("offline")}
	result, err := (Service{Catalog: catalog, Root: root}).Check(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Reports) != 1 || result.Reports[0].Status != StatusUnknown || result.HadErrors {
		t.Fatalf("result = %+v", result)
	}

	writeInstallation(t, root, "tracked.zip", variation(3, downloadable("tracked.zip", "1 MB", 1)))
	result, err = (Service{Catalog: catalog, Root: root}).Check(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Reports) != 2 || !result.HadErrors {
		t.Fatalf("result = %+v", result)
	}
}

func TestServiceReportsRemovedVariation(t *testing.T) {
	root := t.TempDir()
	writeInstallation(t, root, "asset.zip", variation(3, downloadable("asset.zip", "1 MB", 1)))
	catalog := &catalogStub{items: map[int64]importer.ItemMetadata{1: {
		ItemID: 1, ItemName: "Item", ShopID: 2, ShopName: "Shop",
	}}}
	result, err := (Service{Catalog: catalog, Root: root}).Check(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Reports) != 1 || result.Reports[0].Changes[0] != "variation removed" ||
		len(result.Reports[0].LocalDownloadables) != 1 || result.Reports[0].LocalDownloadables[0] != "asset.zip" {
		t.Fatalf("result = %+v", result)
	}
}

func writeInstallation(t *testing.T, root, filename string, snapshot importer.VariationMetadata) {
	t.Helper()
	metadata := importer.InstallationMetadata{
		SchemaVersion: importer.InstallationSchemaVersion,
		Item:          importer.ItemMetadata{ItemID: 1, ItemName: "Item", ShopID: 2, ShopName: "Shop"},
		VariationID:   3, VariationName: "Standard", DownloadableFilename: filename,
		InstalledAt: time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC), Snapshot: &snapshot,
	}
	data, err := json.Marshal(metadata)
	if err != nil {
		t.Fatal(err)
	}
	state := filepath.Join(root, "items", "2_Shop", "1_Item", filename, ".omnooth")
	if err := os.MkdirAll(state, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(state, "installation.json"), data, 0o600); err != nil {
		t.Fatal(err)
	}
}

func variation(id int64, values ...importer.DownloadableMetadata) importer.VariationMetadata {
	return importer.VariationMetadata{ID: id, Name: "Standard", Type: "digital", Downloadables: values}
}

func downloadable(name, size string, day int) importer.DownloadableMetadata {
	stamp := time.Date(2026, 8, day, 0, 0, 0, 0, time.UTC)
	return importer.DownloadableMetadata{Name: name, FileSize: size, CreatedAt: stamp, UpdatedAt: stamp}
}
