# Deterministic icon generator for the Foundry32 workspace. Produces three
# multi-resolution .ico files with distinct classic-flavored motifs:
#   Foundry32   -> a hot anvil (the forge/foundry)
#   MCP Console -> a ">_" terminal prompt
#   WITN        -> a magnifier over a node hexagon (finding the node)
# Run from anywhere: powershell -ExecutionPolicy Bypass -File assets\generate-icons.ps1
Add-Type -AssemblyName System.Drawing

function Argb([int]$a, [int]$r, [int]$g, [int]$b) {
    return [System.Drawing.Color]::FromArgb($a, $r, $g, $b)
}

# Rounded background plate (the classic "app tile" look).
function Draw-Plate($g, [float]$s, $color) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $r = $s * 0.18
    $path.AddArc(0, 0, $r * 2, $r * 2, 180, 90)
    $path.AddArc($s - $r * 2, 0, $r * 2, $r * 2, 270, 90)
    $path.AddArc($s - $r * 2, $s - $r * 2, $r * 2, $r * 2, 0, 90)
    $path.AddArc(0, $s - $r * 2, $r * 2, $r * 2, 90, 90)
    $path.CloseFigure()
    $brush = New-Object System.Drawing.SolidBrush($color)
    $g.FillPath($brush, $path)
    $brush.Dispose(); $path.Dispose()
}

# Foundry32: an anvil silhouette in hot amber, with a spark.
$DrawFoundry = {
    param($g, [float]$s)
    Draw-Plate $g $s (Argb 255 30 26 24)
    $anvil = New-Object System.Drawing.Drawing2D.GraphicsPath
    $pts = @(
        @(0.11, 0.40), @(0.22, 0.34), @(0.84, 0.34), @(0.84, 0.44),
        @(0.64, 0.45), @(0.60, 0.47), @(0.585, 0.60), @(0.78, 0.62),
        @(0.78, 0.72), @(0.22, 0.72), @(0.22, 0.62), @(0.415, 0.60),
        @(0.40, 0.47), @(0.20, 0.45), @(0.17, 0.40)
    )
    $poly = foreach ($p in $pts) { New-Object System.Drawing.PointF(($p[0] * $s), ($p[1] * $s)) }
    $anvil.AddPolygon([System.Drawing.PointF[]]$poly)
    $amber = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF(0, ($s * 0.34))),
        (New-Object System.Drawing.PointF(0, ($s * 0.72))),
        (Argb 255 255 176 82), (Argb 255 214 108 32))
    $g.FillPath($amber, $anvil)
    $amber.Dispose(); $anvil.Dispose()
    # spark
    $spark = New-Object System.Drawing.SolidBrush((Argb 255 255 236 180))
    $g.FillEllipse($spark, ($s * 0.72), ($s * 0.24), ($s * 0.07), ($s * 0.07))
    $spark.Dispose()
}

# MCP Console: a ">_" prompt in terminal green on a cool dark plate.
$DrawConsole = {
    param($g, [float]$s)
    Draw-Plate $g $s (Argb 255 22 27 36)
    $green = Argb 255 92 219 132
    $pen = New-Object System.Drawing.Pen($green, ($s * 0.09))
    $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    $chevron = @(
        (New-Object System.Drawing.PointF(($s * 0.30), ($s * 0.34))),
        (New-Object System.Drawing.PointF(($s * 0.52), ($s * 0.50))),
        (New-Object System.Drawing.PointF(($s * 0.30), ($s * 0.66)))
    )
    $g.DrawLines($pen, [System.Drawing.PointF[]]$chevron)
    $pen.Dispose()
    $cursor = New-Object System.Drawing.SolidBrush($green)
    $g.FillRectangle($cursor, ($s * 0.56), ($s * 0.60), ($s * 0.20), ($s * 0.075))
    $cursor.Dispose()
}

# WITN: a node hexagon under a magnifier. The glass is stroked twice — once in
# the plate colour, then in white — so its ring stays readable where it crosses
# the hexagon, down to 16 px.
$DrawWitn = {
    param($g, [float]$s)
    Draw-Plate $g $s (Argb 255 20 28 30)

    $hex = New-Object System.Drawing.Drawing2D.GraphicsPath
    $cx = 0.44; $cy = 0.42; $r = 0.26
    $poly = foreach ($angle in 90, 150, 210, 270, 330, 30) {
        $rad = $angle * [Math]::PI / 180
        New-Object System.Drawing.PointF(
            (($cx + $r * [Math]::Cos($rad)) * $s),
            (($cy - $r * [Math]::Sin($rad)) * $s))
    }
    $hex.AddPolygon([System.Drawing.PointF[]]$poly)
    $green = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF(0, ($s * 0.16))),
        (New-Object System.Drawing.PointF(0, ($s * 0.68))),
        (Argb 255 138 220 130), (Argb 255 74 166 92))
    $g.FillPath($green, $hex)
    $green.Dispose(); $hex.Dispose()

    $glassBox = New-Object System.Drawing.RectangleF(
        ($s * 0.42), ($s * 0.40), ($s * 0.36), ($s * 0.36))
    $handleFrom = New-Object System.Drawing.PointF(($s * 0.735), ($s * 0.735))
    $handleTo = New-Object System.Drawing.PointF(($s * 0.88), ($s * 0.88))
    foreach ($stroke in @(
            @{ color = (Argb 255 20 28 30); width = 0.135 },
            @{ color = (Argb 255 244 248 250); width = 0.065 })) {
        $pen = New-Object System.Drawing.Pen($stroke.color, ($s * $stroke.width))
        $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
        $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
        $g.DrawEllipse($pen, $glassBox)
        $g.DrawLine($pen, $handleFrom, $handleTo)
        $pen.Dispose()
    }
}

function Save-Ico([string]$outPath, $drawMotif) {
    $sizes = 16, 24, 32, 48, 64, 256
    $pngs = foreach ($size in $sizes) {
        $bmp = New-Object System.Drawing.Bitmap($size, $size)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.Clear([System.Drawing.Color]::Transparent)
        & $drawMotif $g ([float]$size)
        $g.Dispose()
        $ms = New-Object System.IO.MemoryStream
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        , $ms.ToArray()
    }
    $dir = Split-Path $outPath -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
    $fs = [System.IO.File]::Create($outPath)
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
    Write-Host "written: $outPath"
}

$root = Split-Path $PSScriptRoot -Parent
Save-Ico (Join-Path $root 'crates\foundry32\assets\foundry32.ico') $DrawFoundry
Save-Ico (Join-Path $root 'crates\mcp-console\assets\mcp-console.ico') $DrawConsole
Save-Ico (Join-Path $root 'crates\witn\assets\witn.ico') $DrawWitn
