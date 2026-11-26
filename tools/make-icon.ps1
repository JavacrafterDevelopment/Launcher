# Generates the launcher's icon — the Modern blue rocket on a transparent ground,
# drawn at every size Windows asks for.
#
#   powershell -ExecutionPolicy Bypass -File tools\make-icon.ps1
#
# Writes two files into assets\:
#   launcher.ico   multi-size icon, embedded into the exe by build.rs (MSVC builds)
#   icon-64.rgba   raw 64x64 RGBA pixels, set as the window/taskbar icon at runtime

Add-Type -AssemblyName System.Drawing

$root   = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$assets = Join-Path $root 'assets'
New-Item -ItemType Directory -Force $assets | Out-Null
$outIco  = Join-Path $assets 'launcher.ico'
$outRgba = Join-Path $assets 'icon-64.rgba'
$sizes   = @(16, 20, 24, 32, 40, 48, 64, 128, 256)

$blueBody = [System.Drawing.Color]::FromArgb(255, 45, 129, 255)   # #2D81FF (--accent)
$blueDeep = [System.Drawing.Color]::FromArgb(255, 22, 89, 196)    # #1659C4 (--accent-ink)
$band     = [System.Drawing.Color]::FromArgb(255, 242, 241, 238)  # #F2F1EE (--bg)
$lamp     = [System.Drawing.Color]::FromArgb(255, 255, 255, 255)  # #FFFFFF

function P([float]$x, [float]$y) { New-Object System.Drawing.PointF($x, $y) }

function New-RocketBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g   = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic

    # Authored on a 64x64 grid (matching the UI SVG) and scaled slightly to fill the icon box.
    $k = $size / 64.0
    $g.ScaleTransform($k, $k)
    $g.TranslateTransform(32.0, 32.0)
    $g.ScaleTransform(1.12, 1.12)
    $g.TranslateTransform(-32.0, -28.5)

    $brBody = New-Object System.Drawing.SolidBrush($blueBody)
    $brDeep = New-Object System.Drawing.SolidBrush($blueDeep)
    $brBand = New-Object System.Drawing.SolidBrush($band)
    $brLamp = New-Object System.Drawing.SolidBrush($lamp)

    # Fins.
    $g.FillPolygon($brDeep, @((P 22 33), (P 13 45), (P 13 53), (P 22 46)))
    $g.FillPolygon($brDeep, @((P 42 33), (P 51 45), (P 51 53), (P 42 46)))

    # Body: an ogive nose running down into straight sides.
    $body = New-Object 'System.Drawing.Drawing2D.GraphicsPath'
    $body.AddBezier((P 32 4), (P 40 10.5), (P 42.5 19), (P 42.5 31))
    $body.AddLine((P 42.5 31), (P 42.5 48.5))
    $body.AddLine((P 42.5 48.5), (P 21.5 48.5))
    $body.AddLine((P 21.5 48.5), (P 21.5 31))
    $body.AddBezier((P 21.5 31), (P 21.5 19), (P 24 10.5), (P 32 4))
    $body.CloseFigure()
    $g.FillPath($brBody, $body)

    # Light band, clipped to the body.
    $saved = $g.Save()
    $g.SetClip($body)
    $g.FillRectangle($brBand, 20.0, 38.25, 24.0, 3.5)
    $g.Restore($saved)
    $body.Dispose()

    # Porthole.
    $g.FillEllipse($brDeep, 25.7, 16.7, 12.6, 12.6)
    $g.FillEllipse($brLamp, 28.0, 19.0, 8.0, 8.0)

    # Nozzle.
    $g.FillRectangle($brDeep, 25.5, 47.5, 13.0, 4.2)

    $brBody.Dispose(); $brDeep.Dispose(); $brBand.Dispose(); $brLamp.Dispose()
    $g.Dispose()
    return $bmp
}

function Get-PngBytes($bmp) {
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    # Comma-wrap: without it PowerShell unrolls the array into loose bytes.
    [byte[]]$bytes = $ms.ToArray()
    return ,$bytes
}

$images = New-Object System.Collections.ArrayList
foreach ($s in $sizes) {
    $bmp = New-RocketBitmap $s
    [void]$images.Add([byte[]](Get-PngBytes $bmp))

    if ($s -eq 64) {
        # GDI+ stores BGRA; the window icon wants RGBA, row-major from the top.
        $rect = New-Object System.Drawing.Rectangle(0, 0, 64, 64)
        $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
                              [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $raw = New-Object byte[] (64 * 64 * 4)
        [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $raw, 0, $raw.Length)
        $bmp.UnlockBits($data)
        for ($i = 0; $i -lt $raw.Length; $i += 4) {
            $b = $raw[$i]; $raw[$i] = $raw[$i + 2]; $raw[$i + 2] = $b
        }
        [System.IO.File]::WriteAllBytes($outRgba, $raw)
    }
    $bmp.Dispose()
}

# ICO container: 6-byte header, then one 16-byte directory entry per image.
$fs = [System.IO.File]::Create($outIco)
$bw = New-Object System.IO.BinaryWriter($fs)

$bw.Write([UInt16]0)                 # reserved
$bw.Write([UInt16]1)                 # type: icon
$bw.Write([UInt16]$sizes.Count)

$offset = 6 + (16 * $sizes.Count)
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    $dim = 0
    if ($s -lt 256) { $dim = $s }    # 0 means 256 in the ICO directory
    $bw.Write([Byte]$dim)            # width
    $bw.Write([Byte]$dim)            # height
    $bw.Write([Byte]0)               # palette size
    $bw.Write([Byte]0)               # reserved
    $bw.Write([UInt16]1)             # colour planes
    $bw.Write([UInt16]32)            # bits per pixel
    $bw.Write([UInt32]$images[$i].Length)
    $bw.Write([UInt32]$offset)
    $offset += $images[$i].Length
}

foreach ($img in $images) { $bw.Write([byte[]]$img, 0, $img.Length) }

$bw.Flush(); $bw.Close(); $fs.Close()
Write-Output "Wrote $outIco ($([Math]::Round((Get-Item $outIco).Length / 1KB, 1)) KB, $($sizes.Count) sizes) and $outRgba"
