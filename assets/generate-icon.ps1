# Deterministic icon generator for MCP Hangar — dark plate + hangar dome + door.
Add-Type -AssemblyName System.Drawing

function New-HangarBitmap([int]$Size) {
    $bmp = New-Object System.Drawing.Bitmap($Size, $Size)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)

    $s = [float]$Size
    $plate = New-Object System.Drawing.Drawing2D.GraphicsPath
    $r = $s * 0.18
    $plate.AddArc(0, 0, $r * 2, $r * 2, 180, 90)
    $plate.AddArc($s - $r * 2, 0, $r * 2, $r * 2, 270, 90)
    $plate.AddArc($s - $r * 2, $s - $r * 2, $r * 2, $r * 2, 0, 90)
    $plate.AddArc(0, $s - $r * 2, $r * 2, $r * 2, 90, 90)
    $plate.CloseFigure()
    $bgBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 31, 38, 51))
    $g.FillPath($bgBrush, $plate)

    $dome = New-Object System.Drawing.Drawing2D.GraphicsPath
    $left = $s * 0.16; $right = $s * 0.84; $base = $s * 0.78; $top = $s * 0.24
    $dome.AddArc($left, $top, $right - $left, ($base - $top) * 2, 180, 180)
    $dome.CloseFigure()
    $domeBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 91, 140, 255))
    $g.FillPath($domeBrush, $dome)

    $doorW = $s * 0.24; $doorH = $s * 0.28
    $doorBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 22, 27, 36))
    $g.FillRectangle($doorBrush, ($s - $doorW) / 2, $base - $doorH, $doorW, $doorH)

    $g.Dispose()
    return $bmp
}

$sizes = 16, 24, 32, 48, 64, 256
$pngs = foreach ($size in $sizes) {
    $bmp = New-HangarBitmap $size
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , $ms.ToArray()
}

$out = Join-Path $PSScriptRoot 'hangar.ico'
$fs = [System.IO.File]::Create($out)
$w = New-Object System.IO.BinaryWriter($fs)
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $dim = if ($sizes[$i] -eq 256) { 0 } else { $sizes[$i] }
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$pngs[$i].Length); $w.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($png in $pngs) { $w.Write($png) }
$w.Dispose(); $fs.Dispose()
Write-Host "written: $out"
