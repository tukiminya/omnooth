package importer

import (
	"errors"
	"fmt"
	"net/url"
	"strconv"
	"strings"
)

var ErrInvalidImportURI = errors.New("invalid BOOTH import URI")

var requiredQueryParameters = []string{
	"dlurl",
	"downloadable_filename",
	"item_id",
	"variation_id",
}

// ParseImportURI は、BOOTH Library Managerの取込URIを検証して解析する。
// dlurlには署名情報が含まれる可能性があるため、返すエラーには入力URIを含めない。
func ParseImportURI(raw string) (ImportRequest, error) {
	u, err := url.Parse(raw)
	if err != nil || u.Opaque != "" {
		return ImportRequest{}, ErrInvalidImportURI
	}
	if !strings.EqualFold(u.Scheme, "booth-library-manager") ||
		!strings.EqualFold(u.Host, "item-import") ||
		u.User != nil || u.Port() != "" ||
		(u.Path != "" && u.Path != "/") || u.Fragment != "" {
		return ImportRequest{}, ErrInvalidImportURI
	}

	query := u.Query()
	for _, key := range requiredQueryParameters {
		values, ok := query[key]
		if !ok || len(values) != 1 || strings.TrimSpace(values[0]) == "" {
			return ImportRequest{}, fmt.Errorf("%w: %s must appear exactly once", ErrInvalidImportURI, key)
		}
	}

	downloadURL, err := url.Parse(query.Get("dlurl"))
	if err != nil {
		return ImportRequest{}, fmt.Errorf("%w: invalid dlurl", ErrInvalidImportURI)
	}
	if err := ValidateDownloadURL(downloadURL); err != nil {
		return ImportRequest{}, err
	}

	itemID, err := positiveID(query.Get("item_id"), "item_id")
	if err != nil {
		return ImportRequest{}, err
	}
	var orderID int64
	if values, ok := query["order_id"]; ok {
		if len(values) != 1 {
			return ImportRequest{}, fmt.Errorf("%w: order_id must not appear more than once", ErrInvalidImportURI)
		}
		if strings.TrimSpace(values[0]) != "" {
			orderID, err = positiveID(values[0], "order_id")
			if err != nil {
				return ImportRequest{}, err
			}
		}
	}
	variationID, err := positiveID(query.Get("variation_id"), "variation_id")
	if err != nil {
		return ImportRequest{}, err
	}

	return ImportRequest{
		DownloadURL:          downloadURL,
		DownloadableFilename: query.Get("downloadable_filename"),
		ItemID:               itemID,
		OrderID:              orderID,
		VariationID:          variationID,
	}, nil
}

func positiveID(value, name string) (int64, error) {
	id, err := strconv.ParseInt(value, 10, 64)
	if err != nil || id <= 0 {
		return 0, fmt.Errorf("%w: %s must be a positive integer", ErrInvalidImportURI, name)
	}
	return id, nil
}
