<#
.SYNOPSIS
    Architecture Guidelines Linter for Peregrine
.DESCRIPTION
    Checks the codebase for violations of architectural guidelines:
    - Memory allocations in simulation hot paths
    - Non-deterministic operations (HashMap iteration, thread_rng, etc.)
    - Hardcoded magic numbers
    - File size limits
    - Non-preallocated dynamic structures (Vec::new, HashMap::new, etc.)
    - Non-fixed capacity data structures (Vec, HashMap, String, etc.)
    - Constant computations that should be extracted
    - ECS component add/remove operations (archetype stability)
    
    Memory Safety Guidelines:
    - All collections must be created with capacity OR
    - Must be proven to never grow after initialization OR  
    - Should use frozen/immutable alternatives where possible
    - Use // MEMORY_OK: <reason> to suppress false positives
    
    Performance Guidelines:
    - Prefer fixed-capacity types (Box<[T]>, ArrayVec) over dynamic (Vec<T>)
    - Extract constant computations to const declarations
    - Use // NO_PERFORMANCE_IMPACT to allow dynamic types when justified
    - Use // NO_CONSTANT_OK to allow inline computations when appropriate
    
    ECS Architecture Guidelines:
    - Avoid adding/removing components at runtime (causes archetype changes)
    - Use state enum fields within components instead of structural changes
    - Use // ECS_HANDLING_OK only for intentional, long-term structural changes
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
# CHECK 7: Memory-Unsafe Dynamic Structures
# ============================================================================
Write-Section "Checking for Non-Preallocated Dynamic Structures"

$dynamicStructViolations = 0

# Patterns for structures that should have capacity or be frozen
$dynamicStructPatterns = @(
    @{ Pattern = 'Vec::new\(\)'; Type = 'Vec'; Message = 'Vec::new() without capacity - use Vec::with_capacity() or prove it never grows after init' }
    @{ Pattern = 'HashMap::new\(\)'; Type = 'HashMap'; Message = 'HashMap::new() without capacity - use HashMap::with_capacity() or replace with frozen structure' }
    @{ Pattern = 'HashSet::new\(\)'; Type = 'HashSet'; Message = 'HashSet::new() without capacity - use HashSet::with_capacity() or replace with frozen structure' }
    @{ Pattern = 'BTreeMap::new\(\)'; Type = 'BTreeMap'; Message = 'BTreeMap::new() without capacity - consider pre-sized or frozen alternative' }
    @{ Pattern = 'BTreeSet::new\(\)'; Type = 'BTreeSet'; Message = 'BTreeSet::new() without capacity - consider pre-sized or frozen alternative' }
    @{ Pattern = 'VecDeque::new\(\)'; Type = 'VecDeque'; Message = 'VecDeque::new() without capacity - use VecDeque::with_capacity()' }
    @{ Pattern = 'String::new\(\)'; Type = 'String'; Message = 'String::new() without capacity - use String::with_capacity() or &str' }
)

# Check for dynamic push/insert patterns
$growthPatterns = @(
    @{ Pattern = '\.push\('; Method = 'push'; Message = 'Dynamic growth via push() - ensure collection is pre-allocated or frozen after init' }
    @{ Pattern = '\.insert\('; Method = 'insert'; Message = 'Dynamic growth via insert() - ensure collection is pre-allocated or frozen after init' }
    @{ Pattern = '\.push_str\('; Method = 'push_str'; Message = 'Dynamic growth via push_str() - consider using fixed-capacity buffer' }
    @{ Pattern = '\.extend\('; Method = 'extend'; Message = 'Dynamic growth via extend() - ensure collection has sufficient capacity' }
    @{ Pattern = '\.append\('; Method = 'append'; Message = 'Dynamic growth via append() - ensure collection has sufficient capacity' }
)

# Files to check (all Rust files, but focus on game logic)
$allRustFiles = Get-ChildItem -Path "src/game" -Filter "*.rs" -Recurse

foreach ($file in $allRustFiles) {
    $content = Get-Content $file.FullName -Raw
    $lines = Get-Content $file.FullName
    
    # Skip test modules
    if ($content -match '#\[cfg\(test\)\]' -or $file.Name -match 'test') {
        continue
    }
    
    # Check for unpreallocated structures
    foreach ($check in $dynamicStructPatterns) {
        $matches = Select-String -Path $file.FullName -Pattern $check.Pattern -AllMatches
        foreach ($match in $matches) {
            $lineContent = $lines[$match.LineNumber - 1].Trim()
            
            # Skip comments
            if ($lineContent -match '^\s*//') {
                continue
            }
            
            # Check for NOLINT or MEMORY_OK comments
            if ($match.LineNumber -gt 1) {
                $prevLine = $lines[$match.LineNumber - 2].Trim()
                if ($prevLine -match '//\s*(NOLINT|MEMORY_OK|OK:)') {
                    continue
                }
            }
            if ($lineContent -match '//\s*(NOLINT|MEMORY_OK|OK:)') {
                continue
            }
            
            # Check if it's in a system or component (more critical for hot paths)
            $contextStart = [Math]::Max(0, $match.LineNumber - 10)
            $contextEnd = [Math]::Min($lines.Count - 1, $match.LineNumber + 2)
            $context = $lines[$contextStart..$contextEnd] -join "`n"
            
            # Flag if in a struct field, resource, or component
            $isCritical = $context -match '\bstruct\s+\w+' -or 
                          $context -match '#\[derive\(.*Component' -or 
                          $context -match '#\[derive\(.*Resource' -or
                          $file.Name -match '(components|resources|systems)\.rs'
            
            if ($isCritical) {
                $relativePath = $file.FullName.Replace("$PWD\\", "")
                Write-Violation -File $relativePath -Line $match.LineNumber -Message $check.Message -Code $lineContent
                $dynamicStructViolations++
            }
        }
    }
    
    # Check for growth operations in simulation paths
    if ($file.FullName -match '(simulation|pathfinding|unit)') {
        foreach ($check in $growthPatterns) {
            $matches = Select-String -Path $file.FullName -Pattern $check.Pattern -AllMatches
            foreach ($match in $matches) {
                $lineContent = $lines[$match.LineNumber - 1].Trim()
                
                # Skip comments
                if ($lineContent -match '^\s*//') {
                    continue
                }
                
                # Check for NOLINT or MEMORY_OK comments
                if ($match.LineNumber -gt 1) {
                    $prevLine = $lines[$match.LineNumber - 2].Trim()
                    if ($prevLine -match '//\s*(NOLINT|MEMORY_OK|OK:)') {
                        continue
                    }
                }
                if ($lineContent -match '//\s*(NOLINT|MEMORY_OK|OK:)') {
                    continue
                }
                
                # Check if it's in a hot path (system function)
                $contextStart = [Math]::Max(0, $match.LineNumber - 10)
                $contextEnd = [Math]::Min($lines.Count - 1, $match.LineNumber + 5)
                $context = $lines[$contextStart..$contextEnd] -join "`n"
                
                if ($context -match '\bfn\s+\w+.*\(.*\b(Query|Res|Local)\b' -or $file.Name -match 'systems\.rs') {
                    $relativePath = $file.FullName.Replace("$PWD\\", "")
                    Write-Warning -File $relativePath -Line $match.LineNumber `
                        -Message "$($check.Message)"
                }
            }
        }
    }
}

if ($dynamicStructViolations -eq 0) {
    Write-Pass "No uncontrolled dynamic structures found"
}

Write-Host ""
Write-Host "TIP: Mark safe dynamic allocations with // MEMORY_OK: <reason> comment" -ForegroundColor Gray
Write-Host "     Examples: 'preallocated with capacity', 'frozen after init', 'setup only'" -ForegroundColor Gray

# ============================================================================
# CHECK 8: Non-Fixed Capacity Data Structures
# ============================================================================
Write-Section "Checking for Non-Fixed Capacity Data Structures"

$nonFixedCapacityViolations = 0

# Patterns for types that should be replaced with fixed-capacity alternatives
$nonFixedCapacityPatterns = @(
    @{ Pattern = ':\s*Vec<([^>]+)>'; Type = 'Vec'; Suggestion = 'Use Box<[T]> for fixed-size data, ArrayVec, or scratch buffers' }
    @{ Pattern = ':\s*HashMap<([^>]+),\s*([^>]+)>'; Type = 'HashMap'; Suggestion = 'Use FrozenMap, BTreeMap with pre-allocation, or scratch buffers' }
    @{ Pattern = ':\s*HashSet<([^>]+)>'; Type = 'HashSet'; Suggestion = 'Use FrozenSet, BTreeSet with pre-allocation, or scratch buffers' }
    @{ Pattern = ':\s*VecDeque<([^>]+)>'; Type = 'VecDeque'; Suggestion = 'Use fixed-capacity ring buffer or ArrayDeque' }
    @{ Pattern = ':\s*String\b'; Type = 'String'; Suggestion = 'Use &str, Box<str>, or SmallString for fixed-length strings' }
    @{ Pattern = ':\s*BTreeMap<([^>]+),\s*([^>]+)>'; Type = 'BTreeMap'; Suggestion = 'Consider if this can be FrozenMap or pre-sized at construction' }
    @{ Pattern = ':\s*BTreeSet<([^>]+)>'; Type = 'BTreeSet'; Suggestion = 'Consider if this can be FrozenSet or pre-sized at construction' }
)

# Check all game logic files
$gameFiles = Get-ChildItem -Path "src/game" -Filter "*.rs" -Recurse

foreach ($file in $gameFiles) {
    $content = Get-Content $file.FullName -Raw
    $lines = Get-Content $file.FullName
    
    # Skip test modules
    if ($content -match '#\[cfg\(test\)\]' -or $file.Name -match 'test') {
        continue
    }
    
    foreach ($check in $nonFixedCapacityPatterns) {
        $matches = Select-String -Path $file.FullName -Pattern $check.Pattern -AllMatches
        foreach ($match in $matches) {
            $lineNum = $match.LineNumber
            $lineContent = $lines[$lineNum - 1].Trim()
            
            # Skip comments
            if ($lineContent -match '^\s*//') {
                continue
            }
            
            # Check for NO_PERFORMANCE_IMPACT escape hatch
            $hasEscapeHatch = $false
            
            # Check current line
            if ($lineContent -match '//\s*NO_PERFORMANCE_IMPACT') {
                $hasEscapeHatch = $true
            }
            
            # Check previous line
            if ($lineNum -gt 1) {
                $prevLine = $lines[$lineNum - 2].Trim()
                if ($prevLine -match '//\s*NO_PERFORMANCE_IMPACT') {
                    $hasEscapeHatch = $true
                }
            }
            
            # Check next line (in case comment is below)
            if ($lineNum -lt $lines.Count) {
                $nextLine = $lines[$lineNum].Trim()
                if ($nextLine -match '//\s*NO_PERFORMANCE_IMPACT') {
                    $hasEscapeHatch = $true
                }
            }
            
            if (-not $hasEscapeHatch) {
                # Only flag if it's in a Component, Resource, or critical struct
                $contextStart = [Math]::Max(0, $lineNum - 5)
                $contextEnd = [Math]::Min($lines.Count - 1, $lineNum + 2)
                $context = $lines[$contextStart..$contextEnd] -join "`n"
                
                $isCritical = $context -match '#\[derive\(.*Component' -or 
                              $context -match '#\[derive\(.*Resource' -or 
                              $context -match 'pub struct' -or
                              $file.Name -match '(components|resources)\.rs'
                
                if ($isCritical) {
                    $relativePath = $file.FullName.Replace("$PWD\\", "")
                    Write-Violation -File $relativePath -Line $lineNum `
                        -Message "Non-fixed capacity type '$($check.Type)' - $($check.Suggestion). Add // NO_PERFORMANCE_IMPACT to suppress." `
                        -Code $lineContent
                    $nonFixedCapacityViolations++
                }
            }
        }
    }
}

if ($nonFixedCapacityViolations -eq 0) {
    Write-Pass "No non-fixed capacity data structures found (or all marked with NO_PERFORMANCE_IMPACT)"
}

# ============================================================================
# CHECK 9: Constant Value Computations & Loop-Invariant Code
# ============================================================================
Write-Section "Checking for Repeated Constant Computations & Loop-Invariant Code"

$constantComputationViolations = 0

# Patterns that suggest a value should be a constant
# Look for mathematical expressions with only literals (no variables from function params)
$constantPatterns = @(
    # Mathematical operations with only numbers
    @{ Pattern = '=\s*\d+\.?\d*\s*[+\-*/]\s*\d+\.?\d*'; Example = '= 100.0 * 0.5' }
    @{ Pattern = '=\s*\d+\.?\d*\s*\.\s*sqrt\(\)'; Example = '= 2.0.sqrt()' }
    @{ Pattern = '=\s*\d+\.?\d*\s*\.\s*powi?\('; Example = '= 2.0.powi(2)' }
    @{ Pattern = '=\s*f32::consts::\w+\s*[+\-*/]'; Example = '= PI * 2.0' }
    @{ Pattern = '=\s*\d+\.?\d*\s*[+\-*/]\s*f32::consts::'; Example = '= 2.0 * PI' }
)

foreach ($file in $gameFiles) {
    $content = Get-Content $file.FullName -Raw
    $lines = Get-Content $file.FullName
    
    # Skip test files (files that are ONLY tests, not files that contain test submodules)
    # Only skip if filename contains 'test' or if the file STARTS with #[cfg(test)] at the top
    if ($file.Name -match 'test' -or $content -match '^\s*#\[cfg\(test\)\]') {
        continue
    }
    
    # Track function scope and loop scope
    $inFunction = $false
    $functionStartLine = 0
    $braceDepth = 0
    $inLoop = $false
    $loopStartLine = 0
    $loopBraceDepth = 0
    
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        $trimmed = $line.Trim()
        
        # Track function entry
        if ($trimmed -match '^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:async\s+)?(?:const\s+)?fn\s+\w+') {
            $inFunction = $true
            $functionStartLine = $i + 1
            $braceDepth = 0
        }
        
        # Track loop entry (for, while)
        if ($inFunction -and $trimmed -match '^\s*for\s+') {
            $inLoop = $true
            $loopStartLine = $i + 1
            $loopBraceDepth = $braceDepth
        }
        
        # Track brace depth
        $braceDepth += ($line.ToCharArray() | Where-Object { $_ -eq '{' }).Count
        $braceDepth -= ($line.ToCharArray() | Where-Object { $_ -eq '}' }).Count
        
        # Exit loop when braces close
        if ($inLoop -and $braceDepth -le $loopBraceDepth -and $line -match '}') {
            $inLoop = $false
        }
        
        # Exit function when braces close
        if ($inFunction -and $braceDepth -eq 0 -and $line -match '}') {
            $inFunction = $false
        }
        
        # Skip if not in function or if line is a comment
        if (-not $inFunction -or $trimmed -match '^\s*//' -or $trimmed -eq '') {
            continue
        }
        
        # Skip const declarations (they're already constants!)
        if ($trimmed -match '^\s*const\s+') {
            continue
        }
        
        # Check for NO_CONSTANT_OK escape hatch
        $hasEscapeHatch = $false
        
        if ($trimmed -match '//\s*NO_CONSTANT_OK') {
            $hasEscapeHatch = $true
        }
        
        if ($i -gt 0) {
            $prevLine = $lines[$i - 1].Trim()
            if ($prevLine -match '//\s*NO_CONSTANT_OK') {
                $hasEscapeHatch = $true
            }
        }
        
        if ($hasEscapeHatch) {
            continue
        }
        
        # CHECK 9A: Detect loop-invariant computations (computations inside loops using outer-scope vars)
        if ($inLoop -and $trimmed -match '^\s*let\s+(\w+)\s*=\s*(.+);') {
            $varName = $matches[1]
            $expression = $matches[2].Trim()
            
            # Look for patterns like: x * x, x.field * x.field, x.method(), etc.
            # Common patterns: squared values, method calls on config/resources, etc.
            $isLoopInvariant = $false
            
            # Pattern 1: Simple squaring (x * x)
            if ($expression -match '^(\w+)\s*\*\s*\1$') {
                $isLoopInvariant = $true
            }
            # Pattern 2: Field squaring (config.x * config.x or sim_config.field * sim_config.field)
            elseif ($expression -match '^([\w_]+)\.([\w_]+)\s*\*\s*\1\.\2$') {
                $isLoopInvariant = $true
            }
            # Pattern 3: Method call squaring (x.method() * x.method() - rare but possible)
            elseif ($expression -match '^([\w_]+)\.([\w_]+)\(\)\s*\*\s*\1\.\2\(\)$') {
                $isLoopInvariant = $true
            }
            # Pattern 4: .powi(2) calls (x.powi(2))
            elseif ($expression -match '\.powi\(2\)') {
                $isLoopInvariant = $true
            }
            # Pattern 5: Simple multiplication not involving loop vars (a * b where neither is from loop)
            elseif ($expression -match '^\w+\s*\*\s*\w+$' -and 
                    $expression -notmatch '\b(entity|other_entity|other_pos|other_vel|other_|neighbor|item|elem|i|j|k|idx|index|diff|dist_)\b') {
                $isLoopInvariant = $true
            }
            # Pattern 6: Field access multiplication (config.a * config.b)
            elseif ($expression -match '^([\w_]+)\.([\w_]+)\s*\*\s*' -and 
                    $expression -notmatch '\b(entity|other_entity|other_pos|other_vel|other_|neighbor|item|elem|i|j|k|idx|index|diff|dist_)\b') {
                $isLoopInvariant = $true
            }
            
            if ($isLoopInvariant) {
                # Double-check: Skip if it uses obvious loop iteration variables
                if ($expression -notmatch '\b(entity|other_entity|other_pos|other_vel|other_|neighbor|item|elem|i|j|k|idx|index|diff|dist_)\b') {
                    $relativePath = $file.FullName.Replace("$PWD\", "")
                    Write-Violation -File $relativePath -Line ($i + 1) `
                        -Message "Loop-invariant computation '$varName = $expression' - hoist outside loop for performance. Add // NO_CONSTANT_OK to suppress." `
                        -Code $trimmed
                    $constantComputationViolations++
                }
            }
        }
        
        # CHECK 9B: Original check for literal constant computations
        foreach ($check in $constantPatterns) {
            if ($trimmed -match $check.Pattern) {
                # Heuristic: skip if line contains function parameters or self
                # This avoids false positives for computations that depend on inputs
                if ($trimmed -notmatch '\bself\b' -and 
                    $trimmed -notmatch '\w+\s*\.\s*\w+' -and  # Skip method calls on variables
                    $trimmed -notmatch '\w+\[' -and           # Skip array indexing
                    $trimmed -match '(?:let|const)\s+\w+') {  # Must be a variable declaration
                    
                    # Extract the computation
                    $computation = ($trimmed | Select-String -Pattern $check.Pattern).Matches[0].Value
                    
                    $relativePath = $file.FullName.Replace("$PWD\\", "")
                    Write-Violation -File $relativePath -Line ($i + 1) `
                        -Message "Constant computation '$computation' should be extracted to a const. Add // NO_CONSTANT_OK to suppress." `
                        -Code $trimmed
                    $constantComputationViolations++
                    break  # Only report once per line
                }
            }
        }
    }
}

if ($constantComputationViolations -eq 0) {
    Write-Pass "No repeated constant computations or loop-invariant code found (or all marked with NO_CONSTANT_OK)"
}

# ============================================================================
# CHECK 10: ECS Component Add/Remove Operations (Archetype Stability)
# ============================================================================
Write-Section "Checking for ECS Component Add/Remove Operations"

$ecsHandlingViolations = 0

# Patterns for component structural changes that impact ECS performance
$ecsModificationPatterns = @(
    @{ Pattern = '\.insert\s*\(\s*\w+\s*,'; Method = 'insert (component)'; Message = 'Adding component causes archetype change - prefer state enum fields. Structural changes should be rare and long-lasting' }
    @{ Pattern = '\.remove::<[^>]+>\s*\('; Method = 'remove (component)'; Message = 'Removing component causes archetype change - use state field instead. Structural changes should be rare and long-lasting' }
    @{ Pattern = 'commands\.entity\([^)]+\)\.insert\s*\('; Method = 'Commands::insert'; Message = 'Adding component via Commands causes archetype change - prefer state enum fields' }
    @{ Pattern = 'commands\.entity\([^)]+\)\.remove::<'; Method = 'Commands::remove'; Message = 'Removing component via Commands causes archetype change - use state field instead' }
    @{ Pattern = '\.insert_bundle\s*\('; Method = 'insert_bundle'; Message = 'Adding bundle causes archetype change - ensure this is for long-term structural change only' }
    @{ Pattern = '\.remove_bundle::<'; Method = 'remove_bundle'; Message = 'Removing bundle causes archetype change - ensure this is for long-term structural change only' }
)

foreach ($file in $gameFiles) {
    $content = Get-Content $file.FullName -Raw
    $lines = Get-Content $file.FullName
    
    # Skip test modules
    if ($content -match '#\[cfg\(test\)\]' -or $file.Name -match 'test') {
        continue
    }
    
    foreach ($check in $ecsModificationPatterns) {
        $matches = Select-String -Path $file.FullName -Pattern $check.Pattern -AllMatches
        foreach ($match in $matches) {
            $lineNum = $match.LineNumber
            $lineContent = $lines[$lineNum - 1].Trim()
            
            # Skip comments
            if ($lineContent -match '^\s*//') {
                continue
            }
            
            # Check for ECS_HANDLING_OK escape hatch
            $hasEscapeHatch = $false
            
            # Check current line
            if ($lineContent -match '//\s*ECS_HANDLING_OK') {
                $hasEscapeHatch = $true
            }
            
            # Check previous line
            if ($lineNum -gt 1) {
                $prevLine = $lines[$lineNum - 2].Trim()
                if ($prevLine -match '//\s*ECS_HANDLING_OK') {
                    $hasEscapeHatch = $true
                }
            }
            
            # Check next line (in case comment is below)
            if ($lineNum -lt $lines.Count) {
                $nextLine = $lines[$lineNum].Trim()
                if ($nextLine -match '//\s*ECS_HANDLING_OK') {
                    $hasEscapeHatch = $true
                }
            }
            
            if (-not $hasEscapeHatch) {
                # Flag all component add/remove operations in game logic
                # These are particularly problematic in simulation/hot paths
                $isCritical = $file.FullName -match '(simulation|pathfinding|unit|hud)' -or 
                              $file.Name -match 'systems\.rs'
                
                if ($isCritical) {
                    $relativePath = $file.FullName.Replace("$PWD\\", "")
                    Write-Violation -File $relativePath -Line $lineNum `
                        -Message "$($check.Message). Add // ECS_HANDLING_OK if this is intentional and long-lasting." `
                        -Code $lineContent
                    $ecsHandlingViolations++
                }
                else {
                    # Warn for non-critical paths
                    $relativePath = $file.FullName.Replace("$PWD\\", "")
                    Write-Warning -File $relativePath -Line $lineNum `
                        -Message "$($check.Message). Add // ECS_HANDLING_OK if this is intentional."
                }
            }
        }
    }
}

if ($ecsHandlingViolations -eq 0) {
    Write-Pass "No problematic ECS component add/remove operations found (or all marked with ECS_HANDLING_OK)"
}

Write-Host ""
Write-Host "TIP: Avoid adding/removing components at runtime. Use state enum fields in existing components instead." -ForegroundColor Gray
Write-Host "     Example: Instead of removing 'Moving' component, use 'MovementState::Idle' field." -ForegroundColor Gray
Write-Host "     Add // ECS_HANDLING_OK only for rare, long-term structural changes (spawn/despawn scenarios)." -ForegroundColor Gray

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
