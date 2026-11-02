using System;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using Windows.Foundation;

namespace DXO.UI.Views;

/// Packet timing graph + raw report viewer.
public sealed partial class DiagnosticsPage : Page
{
    private const int BytesShown = 128; // a BT DS4 report is padded to 547; the data is up front
    private const int BytesPerRow = 16;
    private static readonly TimeSpan HighlightFor = TimeSpan.FromMilliseconds(600);

    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(100) };
    private readonly float[] _samples = new float[500];
    private byte[] _prev = Array.Empty<byte>();
    private readonly DateTime[] _changedAt = new DateTime[BytesShown];

    public DiagnosticsPage()
    {
        InitializeComponent();
        _timer.Tick += (_, _) => Tick();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e) => _timer.Start();
    protected override void OnNavigatedFrom(NavigationEventArgs e) => _timer.Stop();

    private void GraphHost_SizeChanged(object sender, SizeChangedEventArgs e) => Tick(force: true);

    private void Tick(bool force = false)
    {
        var engine = App.Engine;
        if (engine is null) { RateText.Text = "-"; ReportHeader.Text = "The engine isn't running."; return; }
        if (PauseButton.IsChecked == true && !force) return;

        var t = engine.GetTiming(_samples, out int n);
        bool live = App.Device.Connected && n > 1;

        RateText.Text = live && t.AvgMs > 0 ? $"{1000 / t.AvgMs:0} Hz" : "-";
        AvgText.Text = live ? $"{t.AvgMs:0.00} ms" : "-";
        JitterText.Text = live ? $"±{t.JitterMs:0.00} ms" : "-";
        RangeText.Text = live ? $"{t.MinMs:0.0} / {t.MaxMs:0.0}" : "-";
        LateText.Text = live ? $"{t.Late}" : "-";
        LateText.Foreground = (Brush)Application.Current.Resources[t.Late > n / 50 ? "DxoWarning" : "DxoSuccess"];

        DrawGraph(n, t.AvgMs, t.MaxMs);
        if (PauseButton.IsChecked != true) DrawReport(engine.LastReport(), t.ReportsTotal);
    }

    private void DrawGraph(int n, float avg, float max)
    {
        double w = GraphHost.ActualWidth, h = GraphHost.ActualHeight;
        var pts = new PointCollection();
        if (n < 2 || w <= 0 || h <= 0)
        {
            Trace.Points = pts;
            AvgLine.Visibility = Visibility.Collapsed;
            ScaleTop.Text = AvgLabel.Text = "";
            return;
        }
        // Scale so normal traffic sits in the lower half and spikes stay visible.
        double top = Math.Max(Math.Max(avg * 3, max * 1.1), 1);
        double dx = w / (_samples.Length - 1);
        double x0 = w - (n - 1) * dx; // newest at the right edge
        for (int i = 0; i < n; i++)
            pts.Add(new Point(x0 + i * dx, h - Math.Min(_samples[i] / top, 1) * h));
        Trace.Points = pts;

        double ay = h - avg / top * h;
        AvgLine.X1 = 0; AvgLine.X2 = w; AvgLine.Y1 = AvgLine.Y2 = ay;
        AvgLine.Visibility = Visibility.Visible;
        ScaleTop.Text = $"{top:0.0} ms";
        AvgLabel.Text = $"average {avg:0.00} ms";
    }

    private void DrawReport(byte[] r, uint total)
    {
        if (r.Length == 0)
        {
            ReportHeader.Text = "No reports yet. Connect a controller.";
            HexDump.Inlines.Clear();
            return;
        }
        string kind = r[0] switch
        {
            0x01 => "USB / basic Bluetooth input",
            0x11 => "DualShock 4 Bluetooth input",
            0x31 => "DualSense Bluetooth input",
            _ => "input",
        };
        ReportHeader.Text = $"Report 0x{r[0]:X2} ({kind})  ·  {r.Length} bytes" +
                            (r.Length > BytesShown ? $", first {BytesShown} shown" : "") + $"  ·  #{total}";

        var now = DateTime.UtcNow;
        int count = Math.Min(r.Length, BytesShown);
        for (int i = 0; i < count; i++)
            if (i >= _prev.Length || _prev[i] != r[i]) _changedAt[i] = now;
        _prev = r;

        var accent = (Brush)Application.Current.Resources["DxoAccent"];
        var dim = (Brush)Application.Current.Resources["DxoTextSecondary"];
        HexDump.Inlines.Clear();
        for (int row = 0; row < count; row += BytesPerRow)
        {
            HexDump.Inlines.Add(new Run { Text = $"{row:X3}  ", Foreground = dim });
            for (int i = row; i < Math.Min(row + BytesPerRow, count); i++)
            {
                bool hot = now - _changedAt[i] < HighlightFor;
                var run = new Run { Text = $"{r[i]:X2} " };
                if (hot) { run.Foreground = accent; run.FontWeight = Microsoft.UI.Text.FontWeights.Bold; }
                HexDump.Inlines.Add(run);
            }
            if (row + BytesPerRow < count) HexDump.Inlines.Add(new LineBreak());
        }
    }
}
