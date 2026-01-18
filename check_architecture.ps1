<#
.SYNOPSIS
    Architecture Guidelines Linter for Peregrine
.DESCRIPTION
    Checks the codebase for violations of architectural guidelines:
    - Memory allocations in simulation hot paths
    - Non-deterministic operations (HashMap iteration, thread_rng, etc.)
    - Hardcoded magic numbers
    - File size limits
.PARAMETER Verbose
    Show detailed information about checks
#>

param(
    [switch]$Verbose
)

$ErrorActionPreference = "Stop"
$script:ViolationCount = 0
$script:WarningCount = 0

function Write-Section {
    param([string]$Title)
    Write-Host "`n=== $Title ===" -ForegroundColor Cyan
}

function Write-Violation {
    param([string]$File, [int]$Line, [string]$Message, [string]$Code = "")
    $script:ViolationCount++
    # VS Code recognizes paths in format: path/to/file.rs:line:column
    $location = if ($Line -gt 0) { "$File`:$Line`:1" } else { $File }
    Write-Host "  [ERROR] $location" -ForegroundColor Red
    Write-Host "    $Message" -ForegroundColor Gray
    if ($Code) {
        Write-Host "    Code: $Code" -ForegroundColor DarkGray
    }
}

function Write-Warning {
    param([string]$File, [int]$Line, [string]$Message)
    $script:WarningCount++
    # VS Code recognizes paths in format: path/to/file.rs:line:column
    $location = if ($Line -gt 0) { "$File`:$Line`:1" } else { $File }
    Write-Host "  [WARN] $location" -ForegroundColor Yellow
    Write-Host "    $Message" -ForegroundColor Gray
}

function Write-Pass {
    param([string]$Message)
    Write-Host "  [OK] $Message" -ForegroundColor Green
}

# ============================================================================
# CHECK 1: Memory Allocations in Hot Paths (Simulation Systems)
# ============================================================================
Write-Section "Checking for Memory Allocations in Hot Paths"

$simulationFiles = Get-ChildItem -Path "src/game/simulation", "src/game/pathfinding", "src/game/unit" -Filter "*.rs" -Recurse -ErrorAction SilentlyContinue

$allocationPatterns = @(
    @{ Pattern = 'Vec::new\(\)'; Message = 'Vec::new() in hot path - use Local<Vec> or with_capacity()' }
    @{ Pattern = 'HashMap::new\(\)'; Message = 'HashMap::new() in hot path - use Local<HashMap> or pre-allocate' }
    @{ Pattern = 'HashSet::new\(\)'; Message = 'HashSet::new() in hot path - use Local<HashSet> or pre-allocate' }
    @{ Pattern = 'BTreeMap::new\(\)'; Message = 'BTreeMap::new() in hot path - consider pre-allocation' }
    @{ Pattern = 'String::from\('; Message = 'String::from() in hot path - avoid string allocations' }
    @{ Pattern = '\.to_string\(\)'; Message = '.to_string() in hot path - avoid string allocations' }
    @{ Pattern = '\.to_vec\(\)'; Message = '.to_vec() allocates - consider reusing buffers' }
)

$hotPathViolations = 0
foreach ($file in $simulationFiles) {
    $content = Get-Content $file.FullName -Raw
    $lines = Get-Content $file.FullName
    
    # Skip if file is a test module
    if ($content -match '#\[cfg\(test\)\]') {
        continue
    }
    
    foreach ($check in $allocationPatterns) {
        $matches = Select-String -Path $file.FullName -Pattern $check.Pattern -AllMatches
        foreach ($match in $matches) {
            $lineContent = $lines[$match.LineNumber - 1].Trim()
            
            # Skip comments and test code
            if ($lineContent -match '^\s*//' -or $lineContent -match '#\[test\]') {
                continue
            }
            
            # Skip if it's in a closure that's not in a query.iter() context (heuristic)
            # This is a simple check - could be made more sophisticated
            $contextStart = [Math]::Max(0, $match.LineNumber - 5)
            $contextEnd = [Math]::Min($lines.Count - 1, $match.LineNumber + 2)
            $context = $lines[$contextStart..$contextEnd] -join "`n"
            
            # If it's inside a system function (has Query or Res parameters)
            if ($context -match '\bQuery\b' -or $context -match '\bResMut?\b' -or $file.Name -match 'systems\.rs') {
                $relativePath = $file.FullName.Replace("$PWD\\", "")
                Write-Violation -File $relativePath -Line $match.LineNumber -Message $check.Message -Code $lineContent
                $hotPathViolations++
            }
        }
    }
}

if ($hotPathViolations -eq 0) {
    Write-Pass "No obvious allocation patterns found in simulation hot paths"
}

# ============================================================================
# CHECK 2: Non-Deterministic Operations
# ============================================================================
Write-Section "Checking for Non-Deterministic Operations"

$nonDetPatterns = @(
    @{ Pattern = 'thread_rng\(\)'; Message = 'thread_rng() is non-deterministic - use seeded RNG from resources' }
    @{ Pattern = 'HashMap::iter\(\)|HashSet::iter\(\)'; Message = 'HashMap/HashSet iteration is non-deterministic - use BTreeMap or sorted Vec' }
    @{ Pattern = 'Time::delta_seconds\(\)'; Message = 'Time::delta_seconds() is non-deterministic in sim - use fixed timestep' }
    @{ Pattern = 'Instant::now\(\)'; Message = 'Instant::now() is non-deterministic - avoid in simulation logic' }
    @{ Pattern = 'SystemTime::now\(\)'; Message = 'SystemTime::now() is non-deterministic - avoid in simulation logic' }
)

$nonDetViolations = 0
foreach ($file in $simulationFiles) {
    $content = Get-Content $file.FullName -Raw
    $lines = Get-Content $file.FullName
    
    foreach ($check in $nonDetPatterns) {
        $matches = Select-String -Path $file.FullName -Pattern $check.Pattern -AllMatches
        foreach ($match in $matches) {
            $lineContent = $lines[$match.LineNumber - 1].Trim()
            
            # Skip comments and NOLINT directives
            if ($lineContent -match '^\s*//') {
                continue
            }
            
            # Check if previous line has NOLINT comment
            if ($match.LineNumber -gt 1) {
                $prevLine = $lines[$match.LineNumber - 2].Trim()
                if ($prevLine -match '//\s*NOLINT') {
                    continue
                }
            }
            
            $relativePath = $file.FullName.Replace("$PWD\\", "")
            Write-Violation -File $relativePath -Line $match.LineNumber -Message $check.Message -Code $lineContent
            $nonDetViolations++
        }
    }
}

if ($nonDetViolations -eq 0) {
    Write-Pass "No non-deterministic operations found in simulation code"
}

# ============================================================================
# CHECK 3: Time-Based Profiling (Should Use Tracy/Bevy Spans)
# ============================================================================
Write-Section "Checking for Time-Based Profiling"

$timeProfilingPatterns = @(
    @{ Pattern = '\.elapsed\(\)'; Message = 'Manual timing with .elapsed() - use Tracy spans or Bevy diagnostic spans instead' }
    @{ Pattern = 'std::time'; Message = 'std::time usage - prefer Tracy/Bevy profiling over manual timing' }
    @{ Pattern = 'Instant::now\(\)'; Message = 'Instant::now() for profiling - use Tracy spans or Bevy diagnostic spans instead' }
)

$allRustFiles = Get-ChildItem -Path "src" -Filter "*.rs" -Recurse
$timeProfilingViolations = 0

foreach ($file in $allRustFiles) {
    $content = Get-Content $file.FullName -Raw
    $lines = Get-Content $file.FullName
    
    # Skip if file is a test module
    if ($content -match '#\[cfg\(test\)\]') {
        continue
    }
    
    foreach ($check in $timeProfilingPatterns) {
        $matches = Select-String -Path $file.FullName -Pattern $check.Pattern -AllMatches
        foreach ($match in $matches) {
            $lineContent = $lines[$match.LineNumber - 1].Trim()
            
            # Skip comments and NOLINT directives
            if ($lineContent -match '^\s*//') {
                continue
            }
            
            # Check if previous line has NOLINT comment
            if ($match.LineNumber -gt 1) {
                $prevLine = $lines[$match.LineNumber - 2].Trim()
                if ($prevLine -match '//\s*NOLINT') {
                    continue
                }
            }
            
            $relativePath = $file.FullName.Replace("$PWD\\", "")
            Write-Violation -File $relativePath -Line $match.LineNumber -Message $check.Message -Code $lineContent
            $timeProfilingViolations++
        }
    }
}

if ($timeProfilingViolations -eq 0) {
    Write-Pass "No manual time-based profiling found"
}

# ============================================================================
# CHECK 4: File Size Limits (300-400 line guideline)
# ============================================================================
Write-Section "Checking File Size Limits"

$allRustFiles = Get-ChildItem -Path "src" -Filter "*.rs" -Recurse
$oversizedFiles = 0

foreach ($file in $allRustFiles) {
    # Skip test files
    if ($file.FullName -match '.*test.*') {
        continue
    }
    
    $lineCount = (Get-Content $file.FullName).Count
    
    if ($lineCount -gt 400) {
        Write-Violation -File $file.FullName.Replace("$PWD\", "") -Line 0 `
            -Message "File has $lineCount lines (limit: 400) - consider splitting"
        $oversizedFiles++
    }
    elseif ($lineCount -gt 300 -and $Verbose) {
        Write-Warning -File $file.FullName.Replace("$PWD\", "") -Line 0 `
            -Message "File has $lineCount lines (approaching 400 line limit)"
    }
}

if ($oversizedFiles -eq 0) {
    Write-Pass "All files under 400 lines"
}

# ============================================================================
# CHECK 5: Magic Numbers in Config-able Systems
# ============================================================================
Write-Section "Checking for Magic Numbers (Heuristic)"

$magicNumberViolations = 0
$systemFiles = Get-ChildItem -Path "src/game" -Filter "systems.rs" -Recurse

foreach ($file in $systemFiles) {
    $lines = Get-Content $file.FullName
    
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        
        # Look for inline numeric literals in calculations (heuristic)
        if ($line -match '\+\s*\d+\.\d+|\*\s*\d+\.\d+' -and $line -notmatch '^\s*//') {
            # Check if it's not 0.0, 1.0, 2.0 (common non-magic values)
            if ($line -notmatch '\b[0-2]\.0\b' -and $line -match '\d+\.\d+') {
                $value = ($line | Select-String -Pattern '\d+\.\d+' -AllMatches).Matches[0].Value
                if ([float]$value -notin @(0.0, 1.0, 2.0, 0.5)) {
                    $relativePath = $file.FullName.Replace("$PWD\\", "")
                    Write-Warning -File $relativePath -Line ($i + 1) `
                        -Message "Possible magic number: $value - verify if this should be configurable"
                    $magicNumberViolations++
                }
            }
        }
    }
}

if ($magicNumberViolations -eq 0) {
    Write-Pass "No obvious magic numbers detected"
}

# ============================================================================
# CHECK 6: Avoid .clone() in Hot Paths (Performance Warning)
# ============================================================================
Write-Section "Checking for Excessive Cloning"

$cloneViolations = 0
foreach ($file in $simulationFiles) {
    $lines = Get-Content $file.FullName
    
    $matches = Select-String -Path $file.FullName -Pattern '\.clone\(\)' -AllMatches
    foreach ($match in $matches) {
        $lineContent = $lines[$match.LineNumber - 1].Trim()
        
        # Skip comments and if it's cloning Copy types or known cheap clones
        if ($lineContent -match '^\s*//' -or $lineContent -match '\.clone\(\).*(Vec2|Position|u32|i32|f32)') {
            continue
        }
        
        # Check if previous line has NOLINT comment
        if ($match.LineNumber -gt 1) {
            $prevLine = $lines[$match.LineNumber - 2].Trim()
            if ($prevLine -match '//\s*NOLINT') {
                continue
            }
        }
        
        $relativePath = $file.FullName.Replace("$PWD\\", "")
        Write-Warning -File $relativePath -Line $match.LineNumber `
            -Message "Clone in hot path - verify this is necessary" 
        $cloneViolations++
    }
}

if ($cloneViolations -eq 0) {
    Write-Pass "No excessive cloning detected"
}

# ============================================================================
# SUMMARY
# ============================================================================
Write-Section "Summary"

Write-Host ""
if ($script:ViolationCount -eq 0 -and $script:WarningCount -eq 0) {
    Write-Host "All checks passed! Your codebase follows the architecture guidelines." -ForegroundColor Green
    exit 0
}
else {
    Write-Host "Found $script:ViolationCount violation(s) and $script:WarningCount warning(s)" -ForegroundColor Red
    Write-Host "Review the violations above and refactor as needed." -ForegroundColor Gray
    Write-Host "See documents/Guidelines/ARCHITECTURE.md for guidance." -ForegroundColor Gray
    exit 1
}
