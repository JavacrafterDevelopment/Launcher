# Generates the launcher's icon — a walnut-and-gilt rocket on a transparent ground,
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

$walnut     = [System.Drawing.Color]::FromArgb(255, 107, 79, 53)
$walnutDeep = [System.Drawing.Color]::FromArgb(255, 74, 53, 36)
$cream      = [System.Drawing.Color]::FromArgb(255, 244, 238, 226)
$gilt       = [System.Drawing.Color]::FromArgb(255, 168, 132, 63)
$lamp       = [System.Drawing.Color]::FromArgb(255, 199, 154, 74)

function P([float]$x, [float]$y) { New-Object System.Drawing.PointF($x, $y) }

function New-RocketBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g   = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic

    # Authored on a 48x48 grid and scaled to the target.
    $k = $size / 48.0
    $g.ScaleTransform($k, $k)

    $brWalnut = New-Object System.Drawing.SolidBrush($walnut)
    $brDeep   = New-Object System.Drawing.SolidBrush($walnutDeep)
    $brCream  = New-Object System.Drawing.SolidBrush($cream)
    $brGilt   = New-Object System.Drawing.SolidBrush($gilt)
    $brLamp   = New-Object System.Drawing.SolidBrush($lamp)

    # Exhaust, drawn first so the nozzle sits over its root.
    $flame = New-Object 'System.Drawing.Drawing2D.GraphicsPath'
    $flame.AddPolygon(@((P 19.5 37), (P 24 47), (P 28.5 37)))
    $g.FillPath($brGilt, $flame)
    $flame.Dispose()

    # Fins.
    $g.FillPolygon($brDeep, @((P 16.5 25), (P 9.5 34), (P 9.5 40), (P 16.5 35)))
    $g.FillPolygon($brDeep, @((P 31.5 25), (P 38.5 34), (P 38.5 40), (P 31.5 35)))

    # Body: an ogive nose running down into straight sides.
    $body = New-Object 'System.Drawing.Drawing2D.GraphicsPath'
    $body.AddBezier((P 24 2.5), (P 30 7.5), (P 32 14), (P 32 23))
    $body.AddLine((P 32 23), (P 32 36.5))
    $body.AddLine((P 32 36.5), (P 16 36.5))
    $body.AddLine((P 16 36.5), (P 16 23))
    $body.AddBezier((P 16 23), (P 16 14), (P 18 7.5), (P 24 2.5))
    $body.CloseFigure()
    $g.FillPath($brWalnut, $body)

    # A cream band, clipped to the body.
    $saved = $g.Save()
    $g.SetClip($body)
    $g.FillRectangle($brCream, 14.0, 29.0, 20.0, 2.8)
    $g.Restore($saved)
    $body.Dispose()

    # Porthole.
    $g.FillEllipse($brDeep, 19.4, 12.4, 9.2, 9.2)
    $g.FillEllipse($brLamp, 21.0, 14.0, 6.0, 6.0)

    # Nozzle.
    $g.FillRectangle($brDeep, 19.0, 36.0, 10.0, 3.2)

    $brWalnut.Dispose(); $brDeep.Dispose(); $brCream.Dispose(); $brGilt.Dispose(); $brLamp.Dispose()
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
