package platform

import (
	"archive/tar"
	"archive/zip"
	"compress/gzip"
	"context"
	"encoding/base64"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestArchiveMaterializerOrdinaryFile(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "source")
	destination := filepath.Join(root, "content")
	os.Mkdir(destination, 0o755)
	os.WriteFile(source, []byte("asset"), 0o600)
	extracted, err := (ArchiveMaterializer{}).Materialize(context.Background(), source, "asset.unitypackage", destination)
	if err != nil {
		t.Fatal(err)
	}
	if extracted {
		t.Fatal("ordinary file was reported as archive")
	}
	content, err := os.ReadFile(filepath.Join(destination, "asset.unitypackage"))
	if err != nil || string(content) != "asset" {
		t.Fatalf("content = %q, err = %v", content, err)
	}
}

func TestArchiveMaterializerZIPAndTarGZ(t *testing.T) {
	for _, test := range []struct {
		name     string
		filename string
		write    func(*testing.T, string)
	}{
		{"zip", "asset.zip", writeZIP},
		{"tar.gz", "asset.tar.gz", writeTarGZ},
	} {
		t.Run(test.name, func(t *testing.T) {
			root := t.TempDir()
			source := filepath.Join(root, "source")
			destination := filepath.Join(root, "content")
			os.Mkdir(destination, 0o755)
			test.write(t, source)
			extracted, err := (ArchiveMaterializer{}).Materialize(context.Background(), source, test.filename, destination)
			if err != nil {
				t.Fatal(err)
			}
			if !extracted {
				t.Fatal("archive was not reported as extracted")
			}
			content, err := os.ReadFile(filepath.Join(destination, "folder", "asset.txt"))
			if err != nil || string(content) != "asset" {
				t.Fatalf("content = %q, err = %v", content, err)
			}
			if _, err := os.Stat(source); !os.IsNotExist(err) {
				t.Fatalf("source archive remains: %v", err)
			}
		})
	}
}

func TestArchiveMaterializer7Zip(t *testing.T) {
	// 通常ファイルを含む、upstream提供の小さな7z fixtureを使用する。
	const fixture = "N3q8ryccAARTpfDIYgAAAAAAAAAgAAAAAAAAAMDMhcxiYXIKZm9vCgAAgTMHrjGYapZFTXUTjwzctMaE+1oPqd0uzZmXHJ6j4QB74vYCpg9q7Ktujb3oJ3hy4W538W7Jb5vgkQYVBSEqe1ACMsErIekjytgvhTh7gy6cjpHQfsAAABcGCAEJWgAHCwEAASMDAQEFXQAQAAAMZgoB3ZHz8QAA"
	root := t.TempDir()
	source := filepath.Join(root, "source")
	destination := filepath.Join(root, "content")
	os.Mkdir(destination, 0o755)
	data, _ := base64.StdEncoding.DecodeString(fixture)
	os.WriteFile(source, data, 0o600)
	extracted, err := (ArchiveMaterializer{}).Materialize(context.Background(), source, "asset.7z", destination)
	if err != nil {
		t.Fatal(err)
	}
	if !extracted {
		t.Fatal("7z was not extracted")
	}
	entries, err := os.ReadDir(destination)
	if err != nil || len(entries) == 0 {
		t.Fatalf("7z entries = %v, err = %v", entries, err)
	}
}

func TestArchiveMaterializerRejectsTraversalAndLinks(t *testing.T) {
	for _, test := range []struct {
		name string
		zip  func(*zip.Writer) error
	}{
		{"traversal", func(writer *zip.Writer) error {
			file, err := writer.Create("../escape.txt")
			if err == nil {
				_, err = file.Write([]byte("escape"))
			}
			return err
		}},
		{"symlink", func(writer *zip.Writer) error {
			header := &zip.FileHeader{Name: "link"}
			header.SetMode(os.ModeSymlink | 0o777)
			file, err := writer.CreateHeader(header)
			if err == nil {
				_, err = file.Write([]byte("target"))
			}
			return err
		}},
	} {
		t.Run(test.name, func(t *testing.T) {
			root := t.TempDir()
			source := filepath.Join(root, "source")
			destination := filepath.Join(root, "content")
			os.Mkdir(destination, 0o755)
			output, _ := os.Create(source)
			writer := zip.NewWriter(output)
			if err := test.zip(writer); err != nil {
				t.Fatal(err)
			}
			writer.Close()
			output.Close()
			if _, err := (ArchiveMaterializer{}).Materialize(context.Background(), source, "asset.zip", destination); err == nil {
				t.Fatal("expected unsafe archive rejection")
			}
		})
	}
}

func TestArchiveMaterializerRejectsTarHardlink(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "source")
	destination := filepath.Join(root, "content")
	os.Mkdir(destination, 0o755)
	output, err := os.Create(source)
	if err != nil {
		t.Fatal(err)
	}
	writer := tar.NewWriter(output)
	if err := writer.WriteHeader(&tar.Header{
		Name: "hardlink", Typeflag: tar.TypeLink, Linkname: "target", Mode: 0o644,
	}); err != nil {
		t.Fatal(err)
	}
	writer.Close()
	output.Close()
	if _, err := (ArchiveMaterializer{}).Materialize(context.Background(), source, "asset.tar", destination); err == nil {
		t.Fatal("expected hardlink rejection")
	}
}

func TestSafeArchivePathRejectsPortableEscapes(t *testing.T) {
	for _, name := range []string{"../file", `..\file`, "/absolute", `C:\file`, "file:stream"} {
		if _, err := safeArchivePath(name); err == nil {
			t.Errorf("safeArchivePath(%q) accepted an unsafe path", name)
		}
	}
}

func TestArchiveMaterializerRejectsEncrypted7Zip(t *testing.T) {
	const fixture = "N3q8ryccAAQfQXHHwAAAAAAAAAAoAAAAAAAAALn7J1qgRMFFatX0e5vxiYNkCc0bOOTUmvi+KatqrtUARthD6ik2mQ2RgdM8A3HxtXiuzmUYq53Om8X6sE3kZ+A1br2Ylv2nvh3rUMcWgf1i+MC8UXkcAiQZme6XopM71m+OeNlu8lfFYkKp/EA4SKNOVdtinaYnj0Y6pRJQJhRTVRWX9XgSnN3fd0sFwKmndH7i0WMdM0gRC4Y6c4wSthZkV1kllHvYvgcInoSnXefRgKBVaQ34kCZCq0xo2g90+JSboO8XBiABCYCgAAcLAQABJAbxBwEKUwf0wep1D5nnYwyAlgoB8PBMOwAA"
	root := t.TempDir()
	source := filepath.Join(root, "source")
	destination := filepath.Join(root, "content")
	os.Mkdir(destination, 0o755)
	data, _ := base64.StdEncoding.DecodeString(fixture)
	os.WriteFile(source, data, 0o600)
	_, err := (ArchiveMaterializer{}).Materialize(context.Background(), source, "encrypted.7z", destination)
	if !errors.Is(err, ErrEncryptedArchive) {
		t.Fatalf("error = %v, want ErrEncryptedArchive", err)
	}
}

func TestArchiveMaterializerRecognizesRAR(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "source")
	destination := filepath.Join(root, "content")
	os.Mkdir(destination, 0o755)
	// RAR5シグネチャの後に、意図的に不完全なヘッダーを置く。
	// 通常ファイルとしてコピーされず、RAR展開処理へ渡されて拒否されることを確認する。
	os.WriteFile(source, []byte{'R', 'a', 'r', '!', 0x1a, 0x07, 0x01, 0x00}, 0o600)
	extracted, err := (ArchiveMaterializer{}).Materialize(context.Background(), source, "asset.rar", destination)
	if err == nil || extracted {
		t.Fatalf("expected corrupt RAR extraction failure, extracted=%v err=%v", extracted, err)
	}
	if _, statErr := os.Stat(filepath.Join(destination, "asset.rar")); !os.IsNotExist(statErr) {
		t.Fatalf("RAR was copied as an ordinary file: %v", statErr)
	}
}

func writeZIP(t *testing.T, filename string) {
	t.Helper()
	output, err := os.Create(filename)
	if err != nil {
		t.Fatal(err)
	}
	writer := zip.NewWriter(output)
	file, err := writer.Create("folder/asset.txt")
	if err == nil {
		_, err = file.Write([]byte("asset"))
	}
	if err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err := output.Close(); err != nil {
		t.Fatal(err)
	}
}

func writeTarGZ(t *testing.T, filename string) {
	t.Helper()
	output, err := os.Create(filename)
	if err != nil {
		t.Fatal(err)
	}
	gzipWriter := gzip.NewWriter(output)
	tarWriter := tar.NewWriter(gzipWriter)
	content := []byte("asset")
	if err := tarWriter.WriteHeader(&tar.Header{Name: "folder/asset.txt", Mode: 0o644, Size: int64(len(content))}); err != nil {
		t.Fatal(err)
	}
	if _, err := tarWriter.Write(content); err != nil {
		t.Fatal(err)
	}
	if err := tarWriter.Close(); err != nil {
		t.Fatal(err)
	}
	if err := gzipWriter.Close(); err != nil {
		t.Fatal(err)
	}
	if err := output.Close(); err != nil {
		t.Fatal(err)
	}
}
