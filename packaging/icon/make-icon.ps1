<#
.SYNOPSIS
  Renders the CAD360 application icon and writes crates/cad-app/assets/cad360.ico (16-256 px,
  PNG-compressed entries) plus cad360-256.png (the window icon).

.DESCRIPTION
  The glyph is CAD360's own: an isometric cube (three shaded faces) inside an orange orbit
  ring, the "360" of the name, on a dark rounded tile. Every size is drawn from vectors at that
  size (no downscaling), so the small sizes stay crisp. Re-run after changing the design:

    pwsh -File packaging/icon/make-icon.ps1
#>
param([string]$OutDir = (Join-Path $PSScriptRoot '..\..\crates\cad-app\assets'))

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

function New-Color([string]$hex) { [System.Drawing.ColorTranslator]::FromHtml($hex) }

function Get-RoundedRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = 2 * $r
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    $p
}

function Render-Icon([int]$S) {
    $bmp = New-Object System.Drawing.Bitmap($S, $S, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    # tile
    $m = [Math]::Max(0.5, $S * 0.03)
    $tile = Get-RoundedRect $m $m ($S - 2 * $m) ($S - 2 * $m) ($S * 0.2)
    $grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush((New-Object System.Drawing.PointF(0, 0)), (New-Object System.Drawing.PointF(0, $S)), (New-Color '#2b3647'), (New-Color '#141920'))
    $g.FillPath($grad, $tile)
    if ($S -ge 32) {
        $g.DrawPath((New-Object System.Drawing.Pen((New-Color '#3e4b5e'), [Math]::Max(1, $S / 96))), $tile)
    }

    # orbit ring (back half behind the cube, front half over it)
    $cx = $S * 0.5; $cy = $S * 0.53
    $ringW = $S * 0.80; $ringH = $S * 0.30
    $ringPen = New-Object System.Drawing.Pen((New-Color '#ff8a3d'), [Math]::Max(1.2, $S * 0.05))
    $ringPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $ringPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $state = $g.Save()
    $g.TranslateTransform($cx, $cy)
    $g.RotateTransform(-18)
    $g.DrawArc($ringPen, -$ringW / 2, -$ringH / 2, $ringW, $ringH, 190, 160)
    $g.Restore($state)

    # isometric cube
    $e = $S * 0.27
    $k = [Math]::Cos([Math]::PI / 6) * $e
    $P = { param($x, $y) New-Object System.Drawing.PointF($x, $y) }
    $T = & $P $cx ($cy - $e); $UR = & $P ($cx + $k) ($cy - $e / 2); $LR = & $P ($cx + $k) ($cy + $e / 2)
    $B = & $P $cx ($cy + $e); $LL = & $P ($cx - $k) ($cy + $e / 2); $UL = & $P ($cx - $k) ($cy - $e / 2)
    $C = & $P $cx $cy
    $g.FillPolygon((New-Object System.Drawing.SolidBrush((New-Color '#9fd0ff'))), [System.Drawing.PointF[]]@($T, $UR, $C, $UL))
    $g.FillPolygon((New-Object System.Drawing.SolidBrush((New-Color '#4d9cff'))), [System.Drawing.PointF[]]@($UL, $C, $B, $LL))
    $g.FillPolygon((New-Object System.Drawing.SolidBrush((New-Color '#2465c7'))), [System.Drawing.PointF[]]@($C, $UR, $LR, $B))
    if ($S -ge 48) {
        $edge = New-Object System.Drawing.Pen((New-Color '#e8f3ff'), [Math]::Max(1, $S / 128))
        $edge.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
        $g.DrawPolygon($edge, [System.Drawing.PointF[]]@($T, $UR, $LR, $B, $LL, $UL))
        $g.DrawLines($edge, [System.Drawing.PointF[]]@($UL, $C, $UR))
        $g.DrawLine($edge, $C, $B)
    }

    $state = $g.Save()
    $g.TranslateTransform($cx, $cy)
    $g.RotateTransform(-18)
    $g.DrawArc($ringPen, -$ringW / 2, -$ringH / 2, $ringW, $ringH, 350, 200)
    $g.Restore($state)

    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , $ms.ToArray()
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
$sizes = 16, 24, 32, 48, 64, 128, 256
$pngs = @{}
foreach ($s in $sizes) { $pngs[$s] = Render-Icon $s }

# ICO container: ICONDIR + one ICONDIRENTRY per size, PNG payloads (Windows Vista+)
$ico = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter($ico)
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
foreach ($s in $sizes) {
    $dim = if ($s -ge 256) { 0 } else { $s }
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$pngs[$s].Length); $w.Write([uint32]$offset)
    $offset += $pngs[$s].Length
}
foreach ($s in $sizes) { $w.Write($pngs[$s]) }
$w.Flush()

$icoPath = Join-Path $OutDir 'cad360.ico'
$pngPath = Join-Path $OutDir 'cad360-256.png'
[System.IO.File]::WriteAllBytes($icoPath, $ico.ToArray())
[System.IO.File]::WriteAllBytes($pngPath, $pngs[256])
Write-Host "Wrote $icoPath ($($ico.Length) bytes, sizes $($sizes -join ', '))"
Write-Host "Wrote $pngPath"
