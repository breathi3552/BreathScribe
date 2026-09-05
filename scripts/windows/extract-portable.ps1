<#
.SYNOPSIS
    从 release-builds/p0-base 中的构建产物解包生成免安装绿色便携版。
.DESCRIPTION
    提取 MSI 或 NSIS Setup 安装包中的二进制和静态资源至目标目录，
    写入 portable 标识文件并初始化 Data 目录，确保配置与数据隔离不污染系统环境。
.PARAMETER OutputDir
    便携版输出目录，默认为 release-builds/portable。
.PARAMETER PackagePath
    指定的安装包路径（.msi 或 .exe）。若不指定则自动探测 release-builds/p0-base。
.PARAMETER Force
    强制关闭正在运行的 handy 进程以完成文件覆盖。
#>

[CmdletBinding()]
param (
    [string]$OutputDir = "release-builds/portable",
    [string]$PackagePath = "",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$targetDir = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDir))
$baseDir = Join-Path $repoRoot "release-builds/p0-base"

# 1. 检查运行中进程
$runningProcesses = Get-Process -Name "handy" -ErrorAction SilentlyContinue
if ($runningProcesses) {
    if ($Force) {
        Write-Host "检测到运行中的 handy 进程，正在终止..."
        $runningProcesses | Stop-Process -Force
        Start-Sleep -Milliseconds 500
    } else {
        Write-Warning "检测到 handy.exe 正在运行 (PID: $($runningProcesses.Id -join ', '))，可能导致文件占用。建议添加 -Force 参数或手动关闭应用。"
    }
}

# 2. 定位安装包
if (-not $PackagePath) {
    $msiFiles = Get-ChildItem -Path $baseDir -Filter "*.msi" -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending
    $exeFiles = Get-ChildItem -Path $baseDir -Filter "*setup.exe" -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending

    if ($msiFiles.Count -gt 0) {
        $PackagePath = $msiFiles[0].FullName
    } elseif ($exeFiles.Count -gt 0) {
        $PackagePath = $exeFiles[0].FullName
    } else {
        throw "未在 $baseDir 中找到可用的 .msi 或 setup.exe 安装包。请先确认 CI 产物已同步或通过 main-build 流水线构建。"
    }
} else {
    $PackagePath = [System.IO.Path]::GetFullPath($PackagePath)
}

if (-not (Test-Path $PackagePath)) {
    throw "安装包文件不存在: $PackagePath"
}

Write-Host "[1/4] 选定构建安装包: $PackagePath"

# 3. 解包安装包
$tempExtractDir = Join-Path $repoRoot "release-builds/portable-extract-tmp"
if (Test-Path $tempExtractDir) {
    Remove-Item -Recurse -Force $tempExtractDir
}
New-Item -ItemType Directory -Force -Path $tempExtractDir | Out-Null

if ($PackagePath.EndsWith(".msi", [System.StringComparison]::OrdinalIgnoreCase)) {
    Write-Host "[2/4] 执行 msiexec 静默管理提取..."
    $proc = Start-Process msiexec.exe -ArgumentList @("/a", "`"$PackagePath`"", "/qn", "TARGETDIR=`"$tempExtractDir`"") -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        Remove-Item -Recurse -Force $tempExtractDir -ErrorAction SilentlyContinue
        throw "msiexec 提取失败，退出代码: $($proc.ExitCode)"
    }
    $sourceFilesDir = Join-Path $tempExtractDir "PFiles/Handy Cloud"
    if (-not (Test-Path $sourceFilesDir)) {
        $exeMatch = Get-ChildItem -Path $tempExtractDir -Filter "handy.exe" -Recurse -File | Select-Object -First 1
        if ($exeMatch) {
            $sourceFilesDir = $exeMatch.DirectoryName
        } else {
            Remove-Item -Recurse -Force $tempExtractDir -ErrorAction SilentlyContinue
            throw "MSI 解包产物中未找到 handy.exe"
        }
    }
} else {
    Write-Host "[2/4] 执行 NSIS 便携安装提取..."
    $proc = Start-Process -FilePath $PackagePath -ArgumentList @("/S", "/PORTABLE", "/D=$tempExtractDir") -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        Remove-Item -Recurse -Force $tempExtractDir -ErrorAction SilentlyContinue
        throw "NSIS 便携安装提取失败，退出代码: $($proc.ExitCode)"
    }
    $sourceFilesDir = $tempExtractDir
}

# 4. 部署到目标便携目录并建立隔离标记
Write-Host "[3/4] 同步文件至便携目录: $targetDir"
if (-not (Test-Path $targetDir)) {
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
}

Get-ChildItem -Path $sourceFilesDir | ForEach-Object {
    Copy-Item -Path $_.FullName -Destination $targetDir -Recurse -Force
}

Remove-Item -Recurse -Force $tempExtractDir -ErrorAction SilentlyContinue

$markerFile = Join-Path $targetDir "portable"
Set-Content -Path $markerFile -Value "Handy Portable Mode" -NoNewline -Encoding utf8

$dataDir = Join-Path $targetDir "Data"
if (-not (Test-Path $dataDir)) {
    New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
}

# 5. 验证运行
$handyExe = Join-Path $targetDir "handy.exe"
if (-not (Test-Path $handyExe)) {
    throw "便携目录中未生成 handy.exe: $handyExe"
}

Write-Host "[4/4] 验证便携版本..."
$testProc = Start-Process -FilePath $handyExe -ArgumentList @("--help") -Wait -PassThru -NoNewWindow
if ($testProc.ExitCode -ne 0) {
    throw "handy.exe 执行自检失败，退出码: $($testProc.ExitCode)"
}

Write-Host "`n=========================================="
Write-Host "🎉 便携版解包与部署完成！"
Write-Host "主程序路径: $handyExe"
Write-Host "数据隔离区: $dataDir"
Write-Host "运行验证:   已通过 --help 基础自检"
Write-Host "==========================================`n"
