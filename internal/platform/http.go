package platform

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/tukiminya/omnooth/internal/importer"
)

const (
	defaultCatalogBaseURL = "https://api.booth.pm/vroid/items/"
	maxCatalogResponse    = 4 << 20
)

type HTTPDoer interface {
	Do(*http.Request) (*http.Response, error)
}

// NewHTTPClient は、BOOTHホストの許可リスト外へリダイレクトできないクライアントを返す。
// 正規の配布ファイルは非常に大きい可能性があるためリクエスト全体にはタイムアウトを
// 設けず、接続とレスポンスヘッダーの待機時間だけを制限する。
func NewHTTPClient() *http.Client {
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.DialContext = (&net.Dialer{Timeout: 15 * time.Second, KeepAlive: 30 * time.Second}).DialContext
	transport.TLSHandshakeTimeout = 15 * time.Second
	transport.ResponseHeaderTimeout = 30 * time.Second
	transport.ExpectContinueTimeout = time.Second

	return &http.Client{
		Transport: transport,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) >= 10 {
				return errors.New("too many download redirects")
			}
			return importer.ValidateDownloadURL(req.URL)
		},
	}
}

type BoothCatalog struct {
	Client  HTTPDoer
	BaseURL string
}

func NewBoothCatalog(client HTTPDoer) *BoothCatalog {
	return &BoothCatalog{Client: client, BaseURL: defaultCatalogBaseURL}
}

func (c *BoothCatalog) GetItem(ctx context.Context, itemID int64) (importer.ItemMetadata, error) {
	if itemID <= 0 {
		return importer.ItemMetadata{}, errors.New("item ID must be positive")
	}
	if c.Client == nil {
		return importer.ItemMetadata{}, errors.New("HTTP client is required")
	}
	base := c.BaseURL
	if base == "" {
		base = defaultCatalogBaseURL
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, base+strconv.FormatInt(itemID, 10), nil)
	if err != nil {
		return importer.ItemMetadata{}, fmt.Errorf("create metadata request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "omnooth/1")

	resp, err := c.Client.Do(req)
	if err != nil {
		return importer.ItemMetadata{}, fmt.Errorf("request metadata: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return importer.ItemMetadata{}, fmt.Errorf("metadata service returned HTTP %d", resp.StatusCode)
	}

	var payload struct {
		ID   int64  `json:"id"`
		Name string `json:"name"`
		Shop struct {
			ID   int64  `json:"id"`
			Name string `json:"name"`
		} `json:"shop"`
	}
	decoder := json.NewDecoder(io.LimitReader(resp.Body, maxCatalogResponse+1))
	if err := decoder.Decode(&payload); err != nil {
		return importer.ItemMetadata{}, fmt.Errorf("decode metadata response: %w", err)
	}
	if payload.ID != itemID {
		return importer.ItemMetadata{}, errors.New("metadata response item ID mismatch")
	}
	if payload.Shop.ID <= 0 || strings.TrimSpace(payload.Shop.Name) == "" || strings.TrimSpace(payload.Name) == "" {
		return importer.ItemMetadata{}, errors.New("metadata response is missing required item or shop fields")
	}

	return importer.ItemMetadata{
		ItemID: payload.ID, ItemName: payload.Name,
		ShopID: payload.Shop.ID, ShopName: payload.Shop.Name,
	}, nil
}

type HTTPDownloader struct {
	Client HTTPDoer
}

func (d HTTPDownloader) Download(ctx context.Context, source *url.URL, destination string) error {
	if err := importer.ValidateDownloadURL(source); err != nil {
		return err
	}
	if d.Client == nil {
		return errors.New("HTTP client is required")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, source.String(), nil)
	if err != nil {
		return errors.New("create download request")
	}
	req.Header.Set("User-Agent", "omnooth/1")

	resp, err := d.Client.Do(req)
	if err != nil {
		// net/httpのエラーには署名付きリクエストURLが含まれるため、そのままラップしない。
		return errors.New("download request failed")
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("download service returned HTTP %d", resp.StatusCode)
	}

	output, err := os.OpenFile(destination, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return fmt.Errorf("create temporary download: %w", err)
	}
	remove := true
	defer func() {
		output.Close()
		if remove {
			_ = os.Remove(destination)
		}
	}()
	if _, err := copyWithContext(ctx, output, resp.Body); err != nil {
		return fmt.Errorf("write download: %w", err)
	}
	if err := output.Close(); err != nil {
		return fmt.Errorf("close download: %w", err)
	}
	remove = false
	return nil
}

func copyWithContext(ctx context.Context, destination io.Writer, source io.Reader) (int64, error) {
	buffer := make([]byte, 128*1024)
	var written int64
	for {
		if err := ctx.Err(); err != nil {
			return written, err
		}
		read, readErr := source.Read(buffer)
		if read > 0 {
			n, writeErr := destination.Write(buffer[:read])
			written += int64(n)
			if writeErr != nil {
				return written, writeErr
			}
			if n != read {
				return written, io.ErrShortWrite
			}
		}
		if readErr != nil {
			if errors.Is(readErr, io.EOF) {
				return written, nil
			}
			return written, readErr
		}
	}
}
