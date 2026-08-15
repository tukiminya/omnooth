package outdated

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strconv"
	"strings"

	"github.com/tukiminya/omnooth/internal/importer"
)

type Status string

const (
	StatusOutdated Status = "outdated"
	StatusUnknown  Status = "unknown"
)

type Report struct {
	Status             Status
	ItemID             int64
	ItemName           string
	VariationID        int64
	VariationName      string
	LocalDownloadables []string
	Changes            []string
	Reason             string
}

type Result struct {
	Reports   []Report
	HadErrors bool
}

type Service struct {
	Catalog importer.ItemCatalog
	Root    string
}

type installation struct {
	metadata importer.InstallationMetadata
	legacy   bool
	broken   bool
	itemID   int64
	itemName string
	filename string
	reason   string
}

type groupKey struct {
	itemID      int64
	variationID int64
}

func (s Service) Check(ctx context.Context) (Result, error) {
	if s.Catalog == nil || s.Root == "" {
		return Result{}, errors.New("outdated service is not configured")
	}
	installations, err := scanInstallations(s.Root)
	if err != nil {
		return Result{}, err
	}
	groups := make(map[groupKey][]installation)
	for _, entry := range installations {
		variationID := entry.metadata.VariationID
		if entry.legacy || entry.broken {
			variationID = 0
		}
		groups[groupKey{itemID: entry.itemID, variationID: variationID}] = append(
			groups[groupKey{itemID: entry.itemID, variationID: variationID}], entry,
		)
	}

	keys := make([]groupKey, 0, len(groups))
	for key := range groups {
		keys = append(keys, key)
	}
	sort.Slice(keys, func(i, j int) bool {
		if keys[i].itemID == keys[j].itemID {
			return keys[i].variationID < keys[j].variationID
		}
		return keys[i].itemID < keys[j].itemID
	})

	items := make(map[int64]importer.ItemMetadata)
	itemErrors := make(map[int64]error)
	result := Result{}
	for _, key := range keys {
		entries := groups[key]
		if key.variationID == 0 {
			report := unknownReport(entries, "missing installation metadata; re-import required")
			for _, entry := range entries {
				if entry.broken {
					report.Reason = entry.reason
					result.HadErrors = true
					break
				}
			}
			result.Reports = append(result.Reports, report)
			continue
		}

		allUntracked := true
		for _, entry := range entries {
			if entry.metadata.Snapshot != nil {
				allUntracked = false
				break
			}
		}
		if allUntracked {
			reason := entries[0].metadata.TrackingIssue
			if reason == "" {
				reason = "installation has no catalog snapshot; re-import required"
			}
			result.Reports = append(result.Reports, unknownReport(entries, reason))
			continue
		}

		item, found := items[key.itemID]
		fetchErr, failed := itemErrors[key.itemID]
		if !found && !failed {
			item, fetchErr = s.Catalog.GetItem(ctx, key.itemID)
			if fetchErr != nil {
				itemErrors[key.itemID] = fetchErr
			} else {
				items[key.itemID] = item
			}
		}
		if fetchErr != nil {
			result.HadErrors = true
			result.Reports = append(result.Reports, unknownReport(entries, "catalog request failed"))
			continue
		}

		current, found := findVariation(item.Variations, key.variationID)
		if !found {
			report := baseReport(entries, StatusOutdated)
			report.LocalDownloadables = installationNames(entries)
			report.Changes = []string{"variation removed"}
			result.Reports = append(result.Reports, report)
			continue
		}

		report := baseReport(entries, StatusOutdated)
		changeSet := make(map[string]struct{})
		var unknownEntries []installation
		for _, entry := range entries {
			if entry.metadata.Snapshot == nil {
				unknownEntries = append(unknownEntries, entry)
				continue
			}
			changes := compare(*entry.metadata.Snapshot, current)
			if len(changes) == 0 {
				continue
			}
			report.LocalDownloadables = append(report.LocalDownloadables, entry.filename)
			for _, change := range changes {
				changeSet[change] = struct{}{}
			}
		}
		if len(report.LocalDownloadables) == 0 {
			if len(unknownEntries) > 0 {
				result.Reports = append(result.Reports, unknownReport(unknownEntries, "installation has no catalog snapshot; re-import required"))
			}
			continue
		}
		for _, entry := range unknownEntries {
			report.LocalDownloadables = append(report.LocalDownloadables, entry.filename)
			changeSet["untracked: "+entry.filename] = struct{}{}
		}
		for change := range changeSet {
			report.Changes = append(report.Changes, change)
		}
		sort.Strings(report.Changes)
		slices.Sort(report.LocalDownloadables)
		result.Reports = append(result.Reports, report)
	}
	return result, nil
}

func scanInstallations(root string) ([]installation, error) {
	itemsRoot := filepath.Join(root, "items")
	shops, err := os.ReadDir(itemsRoot)
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("read item library: %w", err)
	}
	var found []installation
	for _, shop := range shops {
		if !shop.IsDir() || shop.Type()&os.ModeSymlink != 0 {
			continue
		}
		shopPath := filepath.Join(itemsRoot, shop.Name())
		items, err := os.ReadDir(shopPath)
		if err != nil {
			return nil, fmt.Errorf("read shop directory: %w", err)
		}
		for _, item := range items {
			if !item.IsDir() || item.Type()&os.ModeSymlink != 0 {
				continue
			}
			itemID, itemName, ok := parseItemDirectory(item.Name())
			if !ok {
				continue
			}
			itemPath := filepath.Join(shopPath, item.Name())
			downloads, err := os.ReadDir(itemPath)
			if err != nil {
				return nil, fmt.Errorf("read item directory: %w", err)
			}
			for _, download := range downloads {
				if !download.IsDir() || download.Type()&os.ModeSymlink != 0 {
					continue
				}
				entry := installation{itemID: itemID, itemName: itemName, filename: download.Name()}
				manifestPath := filepath.Join(itemPath, download.Name(), ".omnooth", "installation.json")
				data, err := os.ReadFile(manifestPath)
				if errors.Is(err, os.ErrNotExist) {
					entry.legacy = true
					found = append(found, entry)
					continue
				}
				if err != nil {
					entry.broken = true
					entry.reason = "cannot read installation metadata"
					found = append(found, entry)
					continue
				}
				if err := json.Unmarshal(data, &entry.metadata); err != nil ||
					entry.metadata.SchemaVersion != importer.InstallationSchemaVersion ||
					entry.metadata.Item.ItemID != itemID || entry.metadata.VariationID <= 0 ||
					entry.metadata.DownloadableFilename == "" || entry.metadata.InstalledAt.IsZero() {
					entry.broken = true
					entry.reason = "invalid installation metadata"
					found = append(found, entry)
					continue
				}
				entry.itemName = entry.metadata.Item.ItemName
				entry.filename = entry.metadata.DownloadableFilename
				found = append(found, entry)
			}
		}
	}
	return found, nil
}

func parseItemDirectory(name string) (int64, string, bool) {
	id, suffix, found := strings.Cut(name, "_")
	if !found {
		return 0, "", false
	}
	itemID, err := strconv.ParseInt(id, 10, 64)
	return itemID, suffix, err == nil && itemID > 0
}

func findVariation(variations []importer.VariationMetadata, id int64) (importer.VariationMetadata, bool) {
	for _, variation := range variations {
		if variation.ID == id {
			return variation, true
		}
	}
	return importer.VariationMetadata{}, false
}

func baseReport(entries []installation, status Status) Report {
	first := entries[0]
	report := Report{
		Status: status, ItemID: first.itemID, ItemName: first.itemName,
		VariationID: first.metadata.VariationID, VariationName: first.metadata.VariationName,
	}
	return report
}

func installationNames(entries []installation) []string {
	values := make([]string, 0, len(entries))
	for _, entry := range entries {
		values = append(values, entry.filename)
	}
	slices.Sort(values)
	return slices.Compact(values)
}

func unknownReport(entries []installation, reason string) Report {
	report := baseReport(entries, StatusUnknown)
	if entries[0].legacy || entries[0].broken {
		report.VariationID = 0
		report.VariationName = ""
	}
	report.LocalDownloadables = installationNames(entries)
	report.Reason = reason
	return report
}

func compare(installed, current importer.VariationMetadata) []string {
	oldByName := groupDownloadables(installed.Downloadables)
	newByName := groupDownloadables(current.Downloadables)
	var changes []string
	for name, oldValues := range oldByName {
		newValues, exists := newByName[name]
		if !exists {
			changes = append(changes, "removed: "+name)
			continue
		}
		if !slices.Equal(oldValues, newValues) {
			changes = append(changes, "modified: "+name)
		}
	}
	for name := range newByName {
		if _, exists := oldByName[name]; !exists {
			changes = append(changes, "added: "+name)
		}
	}
	sort.Strings(changes)
	return changes
}

func groupDownloadables(values []importer.DownloadableMetadata) map[string][]string {
	grouped := make(map[string][]string)
	for _, value := range values {
		fingerprint := value.FileSize + "\x00" + value.CreatedAt.UTC().Format("2006-01-02T15:04:05.999999999Z07:00") +
			"\x00" + value.UpdatedAt.UTC().Format("2006-01-02T15:04:05.999999999Z07:00")
		grouped[value.Name] = append(grouped[value.Name], fingerprint)
	}
	for name := range grouped {
		sort.Strings(grouped[name])
	}
	return grouped
}
