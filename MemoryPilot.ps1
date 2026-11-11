param(
    [switch]$SelfTest,
    [switch]$SmokeTest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName Microsoft.VisualBasic

function Get-MemorySnapshot {
    $info = [Microsoft.VisualBasic.Devices.ComputerInfo]::new()
    $groups = Get-Process | Group-Object ProcessName | ForEach-Object {
        [pscustomobject]@{
            Name = $_.Name
            Count = $_.Count
            WorkingSetMB = [math]::Round((($_.Group | Measure-Object WorkingSet64 -Sum).Sum / 1MB), 0)
        }
    }
    $byName = @{}
    foreach ($group in $groups) { $byName[$group.Name] = $group.WorkingSetMB }

    [pscustomobject]@{
        Time = Get-Date
        TotalMB = [math]::Round($info.TotalPhysicalMemory / 1MB, 0)
        AvailableMB = [math]::Round($info.AvailablePhysicalMemory / 1MB, 0)
        BraveMB = [int]$byName['brave']
        ChatGPTMB = [int]$byName['ChatGPT'] + [int]$byName['codex']
        Groups = @($groups | Sort-Object WorkingSetMB -Descending | Select-Object -First 18)
    }
}

if ($SelfTest) {
    Get-MemorySnapshot | ConvertTo-Json -Depth 4
    return
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[System.Windows.Forms.Application]::EnableVisualStyles()

$form = [System.Windows.Forms.Form]::new()
$form.Text = 'Memory Pilot'
$form.Width = 780
$form.Height = 650
$form.MinimumSize = [System.Drawing.Size]::new(700, 550)
$form.StartPosition = 'CenterScreen'
$form.Font = [System.Drawing.Font]::new('Segoe UI', 10)

$summary = [System.Windows.Forms.Label]::new()
$summary.Location = [System.Drawing.Point]::new(20, 20)
$summary.Size = [System.Drawing.Size]::new(720, 46)
$summary.Anchor = 'Top,Left,Right'
$summary.Font = [System.Drawing.Font]::new('Segoe UI', 13, [System.Drawing.FontStyle]::Bold)
$form.Controls.Add($summary)

$detail = [System.Windows.Forms.Label]::new()
$detail.Location = [System.Drawing.Point]::new(20, 70)
$detail.Size = [System.Drawing.Size]::new(720, 32)
$detail.Anchor = 'Top,Left,Right'
$form.Controls.Add($detail)

$advice = [System.Windows.Forms.Label]::new()
$advice.Location = [System.Drawing.Point]::new(20, 108)
$advice.Size = [System.Drawing.Size]::new(720, 42)
$advice.Anchor = 'Top,Left,Right'
$advice.ForeColor = [System.Drawing.Color]::DarkSlateGray
$form.Controls.Add($advice)

$baselineButton = [System.Windows.Forms.Button]::new()
$baselineButton.Text = 'Запомнить состояние'
$baselineButton.Location = [System.Drawing.Point]::new(20, 160)
$baselineButton.Size = [System.Drawing.Size]::new(190, 36)
$form.Controls.Add($baselineButton)

$braveButton = [System.Windows.Forms.Button]::new()
$braveButton.Text = 'Настроить Brave'
$braveButton.Location = [System.Drawing.Point]::new(220, 160)
$braveButton.Size = [System.Drawing.Size]::new(170, 36)
$form.Controls.Add($braveButton)

$startupButton = [System.Windows.Forms.Button]::new()
$startupButton.Text = 'Автозагрузка Windows'
$startupButton.Location = [System.Drawing.Point]::new(400, 160)
$startupButton.Size = [System.Drawing.Size]::new(200, 36)
$form.Controls.Add($startupButton)

$compare = [System.Windows.Forms.Label]::new()
$compare.Location = [System.Drawing.Point]::new(20, 210)
$compare.Size = [System.Drawing.Size]::new(720, 44)
$compare.Anchor = 'Top,Left,Right'
$compare.Text = 'Нажмите «Запомнить состояние», затем измените настройку и сравните результат.'
$form.Controls.Add($compare)

$table = [System.Windows.Forms.DataGridView]::new()
$table.Location = [System.Drawing.Point]::new(20, 265)
$table.Size = [System.Drawing.Size]::new(720, 300)
$table.Anchor = 'Top,Bottom,Left,Right'
$table.AllowUserToAddRows = $false
$table.AllowUserToDeleteRows = $false
$table.ReadOnly = $true
$table.RowHeadersVisible = $false
$table.SelectionMode = 'FullRowSelect'
$table.AutoSizeColumnsMode = 'Fill'
[void]$table.Columns.Add('Process', 'Процесс')
[void]$table.Columns.Add('Count', 'Копий')
[void]$table.Columns.Add('Memory', 'Память, МБ*')
$table.Columns['Process'].FillWeight = 60
$table.Columns['Count'].FillWeight = 20
$table.Columns['Memory'].FillWeight = 20
$form.Controls.Add($table)

$footnote = [System.Windows.Forms.Label]::new()
$footnote.Location = [System.Drawing.Point]::new(20, 575)
$footnote.Size = [System.Drawing.Size]::new(720, 32)
$footnote.Anchor = 'Bottom,Left,Right'
$footnote.Text = '*Сумма рабочих наборов процессов приблизительна: общая память может учитываться несколько раз.'
$footnote.ForeColor = [System.Drawing.Color]::DimGray
$form.Controls.Add($footnote)

$script:baseline = $null
$script:current = $null
$script:lastError = $null

function Update-View {
    try {
        $script:lastError = $null
        $script:current = Get-MemorySnapshot
        $used = $script:current.TotalMB - $script:current.AvailableMB
        $summary.Text = "Доступно: $($script:current.AvailableMB) МБ из $($script:current.TotalMB) МБ"
        $detail.Text = "Brave: ~$($script:current.BraveMB) МБ    ChatGPT/Codex: ~$($script:current.ChatGPTMB) МБ    Занято всего: ~$used МБ"
        if ($script:current.AvailableMB -lt 1024) {
            $advice.Text = 'Памяти мало. Проверьте тяжёлые вкладки Brave и ненужные приложения ниже.'
            $advice.ForeColor = [System.Drawing.Color]::DarkRed
        } elseif ($script:current.AvailableMB -lt 2048) {
            $advice.Text = 'Память близка к заполнению. Memory Saver Brave может помочь с неактивными вкладками.'
            $advice.ForeColor = [System.Drawing.Color]::DarkOrange
        } else {
            $advice.Text = 'Запас памяти есть. Закрывать процессы ради меньшей цифры сейчас не нужно.'
            $advice.ForeColor = [System.Drawing.Color]::DarkGreen
        }
        $table.Rows.Clear()
        foreach ($group in $script:current.Groups) {
            [void]$table.Rows.Add($group.Name, $group.Count, $group.WorkingSetMB)
        }
        if ($script:baseline) {
            $delta = $script:current.AvailableMB - $script:baseline.AvailableMB
            $compare.Text = "С момента замера: доступной памяти $delta МБ; Brave $($script:current.BraveMB - $script:baseline.BraveMB) МБ; ChatGPT/Codex $($script:current.ChatGPTMB - $script:baseline.ChatGPTMB) МБ."
        }
    } catch {
        $script:lastError = $_.Exception.Message
        $advice.Text = "Ошибка измерения: $($_.Exception.Message)"
        $advice.ForeColor = [System.Drawing.Color]::DarkRed
    }
}

$baselineButton.Add_Click({
    $script:baseline = $script:current
    $compare.Text = "Замер сохранён: $($script:baseline.Time.ToString('HH:mm:ss')). Сравнение обновится через 5 секунд."
})

$braveButton.Add_Click({
    $paths = @(
        "$env:ProgramFiles\BraveSoftware\Brave-Browser\Application\brave.exe",
        "${env:ProgramFiles(x86)}\BraveSoftware\Brave-Browser\Application\brave.exe"
    )
    $browser = $paths | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if ($browser) {
        Start-Process -FilePath $browser -ArgumentList 'brave://settings/performance'
    } else {
        [System.Windows.Forms.MessageBox]::Show('Brave не найден в стандартной папке установки.', 'Memory Pilot') | Out-Null
    }
})

$startupButton.Add_Click({ Start-Process 'ms-settings:startupapps' })

$timer = [System.Windows.Forms.Timer]::new()
$timer.Interval = 5000
$timer.Add_Tick({ Update-View })
$form.Add_Shown({ Update-View; $timer.Start() })
$form.Add_FormClosed({ $timer.Stop(); $timer.Dispose() })
if ($SmokeTest) {
    Update-View
    if ($script:lastError) { throw $script:lastError }
    "UI_OK rows=$($table.Rows.Count) available=$($script:current.AvailableMB)"
    $form.Dispose()
    return
}
[void][System.Windows.Forms.Application]::Run($form)
