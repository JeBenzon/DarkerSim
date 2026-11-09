// DarkerLink — companion for Darker Sim.
//
// Hover an item in Dark and Darker, press the hotkey: DarkerLink screenshots the area around the cursor,
// reads the tooltip with the built-in Windows OCR and sends the text lines (with their colours) to Darker Sim,
// which equips the item on the Attacker (or Target).
//
// It only reads screen pixels — no game memory, no injection, no input sent to the game.
// The game must run in Borderless/Windowed mode for the in-game toast to be visible.

using System.Collections.Concurrent;
using System.Diagnostics;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Net;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Windows.Graphics.Imaging;
using Windows.Media.Ocr;

namespace DarkerLink;

public sealed class Config
{
    public string HotkeyAttacker { get; set; } = "Ctrl+Alt+A";
    public string HotkeyTarget { get; set; } = "Ctrl+Alt+T";
    public int Port { get; set; } = 47321;
    public string SimUrl { get; set; } = "http://127.0.0.1:8080";
    /// <summary>pixels captured around the cursor (the tooltip opens beside it)</summary>
    public int CaptureLeft { get; set; } = 900;
    public int CaptureRight { get; set; } = 900;
    public int CaptureUp { get; set; } = 700;
    public int CaptureDown { get; set; } = 1100;
    public bool SaveCaptures { get; set; } = true;

    public static string Dir => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "DarkerLink");
    public static string FilePath => Path.Combine(Dir, "config.json");
    static readonly JsonSerializerOptions Json = new() { WriteIndented = true };

    public static Config Load()
    {
        Directory.CreateDirectory(Dir);
        try
        {
            if (File.Exists(FilePath)) return JsonSerializer.Deserialize<Config>(File.ReadAllText(FilePath)) ?? new();
        }
        catch { /* fall back to defaults */ }
        var c = new Config();
        File.WriteAllText(FilePath, JsonSerializer.Serialize(c, Json));
        return c;
    }
}

public record OcrLineDto(string text, double x, double y, double w, double h, int r, int g, int b);

public static class Ocr
{
    /// <summary>Runs Windows OCR on a bitmap (upscaled 2× — game fonts are small) and samples each line's text colour.</summary>
    public static double? ScaleOverride;
    public static async Task<List<OcrLineDto>> Read(Bitmap src)
    {
        var engine = OcrEngine.TryCreateFromLanguage(new Windows.Globalization.Language("en-US"))
                     ?? OcrEngine.TryCreateFromUserProfileLanguages()
                     ?? throw new Exception("Windows OCR is not available (install an English language pack)");
        List<(string Text, RectangleF Box)> found;
        if (ScaleOverride is double forced)
            found = await Recognize(engine, src, new Rectangle(0, 0, src.Width, src.Height), forced);
        else
        {
            // Windows OCR returns nothing for big upscaled images with little text, so:
            // pass 1 reads the whole capture at ~1x just to find where the tooltip text is,
            // pass 2 re-reads only that region at 2x (game fonts are small) for accuracy.
            var full = new Rectangle(0, 0, src.Width, src.Height);
            var rough = await Recognize(engine, src, full, Math.Min(1.0, 2000.0 / Math.Max(src.Width, src.Height)));
            found = rough;
            if (rough.Count > 0)
            {
                var u = rough.Select(l => l.Box).Aggregate(RectangleF.Union);
                var crop = Rectangle.Intersect(full, Rectangle.Round(RectangleF.Inflate(u, 40, 40)));
                double scale = Math.Min(2.0, 2400.0 / Math.Max(crop.Width, crop.Height));
                var fine = await Recognize(engine, src, crop, scale);
                if (fine.Count >= rough.Count) found = fine;
            }
        }

        var lines = new List<OcrLineDto>();
        var data = src.LockBits(new Rectangle(0, 0, src.Width, src.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
        try
        {
            foreach (var (text, b) in found)
            {
                var (r, g, bl) = TextColor(data, (int)b.X, (int)b.Y, (int)Math.Ceiling(b.Right), (int)Math.Ceiling(b.Bottom));
                lines.Add(new OcrLineDto(text, Math.Round(b.X, 1), Math.Round(b.Y, 1), Math.Round(b.Width, 1), Math.Round(b.Height, 1), r, g, bl));
            }
        }
        finally { src.UnlockBits(data); }
        return lines;
    }

    /// <summary>OCR one region of the bitmap at the given scale; boxes come back in source-bitmap pixels.</summary>
    static async Task<List<(string Text, RectangleF Box)>> Recognize(OcrEngine engine, Bitmap src, Rectangle region, double scale)
    {
        int w = Math.Max(1, (int)(region.Width * scale)), h = Math.Max(1, (int)(region.Height * scale));
        using var big = new Bitmap(w, h, PixelFormat.Format32bppArgb);
        using (var g = Graphics.FromImage(big))
        {
            g.InterpolationMode = InterpolationMode.HighQualityBicubic;
            g.DrawImage(src, new Rectangle(0, 0, w, h), region, GraphicsUnit.Pixel);
        }
        using var ms = new MemoryStream();
        big.Save(ms, ImageFormat.Png);
        ms.Position = 0;
        var decoder = await BitmapDecoder.CreateAsync(ms.AsRandomAccessStream());
        using var sb = await decoder.GetSoftwareBitmapAsync(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied);
        var result = await engine.RecognizeAsync(sb);
        var outp = new List<(string, RectangleF)>();
        foreach (var line in result.Lines)
        {
            if (line.Words.Count == 0) continue;
            double x0 = line.Words.Min(q => q.BoundingRect.X), y0 = line.Words.Min(q => q.BoundingRect.Y);
            double x1 = line.Words.Max(q => q.BoundingRect.X + q.BoundingRect.Width), y1 = line.Words.Max(q => q.BoundingRect.Y + q.BoundingRect.Height);
            outp.Add((line.Text, new RectangleF((float)(region.X + x0 / scale), (float)(region.Y + y0 / scale), (float)((x1 - x0) / scale), (float)((y1 - y0) / scale))));
        }
        return outp;
    }

    /// <summary>Average of the brightest quarter of pixels in the box = the text colour on a dark tooltip.</summary>
    static (int, int, int) TextColor(BitmapData d, int x0, int y0, int x1, int y1)
    {
        x0 = Math.Clamp(x0, 0, d.Width - 1); x1 = Math.Clamp(x1, 1, d.Width);
        y0 = Math.Clamp(y0, 0, d.Height - 1); y1 = Math.Clamp(y1, 1, d.Height);
        var px = new List<(int l, int r, int g, int b)>();
        unsafe
        {
            for (int y = y0; y < y1; y++)
            {
                byte* row = (byte*)d.Scan0 + y * d.Stride;
                for (int x = x0; x < x1; x++)
                {
                    int b = row[x * 4], g = row[x * 4 + 1], r = row[x * 4 + 2];
                    px.Add((r * 3 + g * 6 + b, r, g, b));
                }
            }
        }
        if (px.Count == 0) return (0, 0, 0);
        var top = px.OrderByDescending(p => p.l).Take(Math.Max(1, px.Count / 4)).ToList();
        return ((int)top.Average(p => p.r), (int)top.Average(p => p.g), (int)top.Average(p => p.b));
    }
}

/// <summary>Tiny localhost server: Darker Sim listens on /events (Server-Sent Events) and posts results to /result.</summary>
public sealed class LinkServer
{
    readonly HttpListener http = new();
    readonly ConcurrentDictionary<int, HttpListenerResponse> clients = new();
    int nextId;
    public event Action<bool, string>? Result;
    public int Clients => clients.Count;

    public LinkServer(int port) => http.Prefixes.Add($"http://localhost:{port}/");

    public void Start()
    {
        http.Start();
        _ = Task.Run(Loop);
        _ = Task.Run(async () =>
        {
            while (true) { await Task.Delay(15000); Broadcast(": ping\n\n"); }
        });
    }

    async Task Loop()
    {
        while (http.IsListening)
        {
            HttpListenerContext ctx;
            try { ctx = await http.GetContextAsync(); } catch { return; }
            _ = Task.Run(() => Handle(ctx));
        }
    }

    async Task Handle(HttpListenerContext ctx)
    {
        var res = ctx.Response;
        res.Headers["Access-Control-Allow-Origin"] = "*";
        // lets the hosted site (https://…) talk to this local companion (Chrome's private/local network access)
        res.Headers["Access-Control-Allow-Private-Network"] = "true";
        try
        {
            var path = ctx.Request.Url?.AbsolutePath ?? "/";
            if (ctx.Request.HttpMethod == "OPTIONS")
            {
                res.Headers["Access-Control-Allow-Methods"] = "GET, POST";
                res.Headers["Access-Control-Allow-Headers"] = "Content-Type";
                res.StatusCode = 204; res.Close(); return;
            }
            if (path == "/events")
            {
                res.ContentType = "text/event-stream";
                res.Headers["Cache-Control"] = "no-cache";
                res.SendChunked = true;
                var id = Interlocked.Increment(ref nextId);
                clients[id] = res;
                Write(res, ": connected\n\n");
                return; // kept open; removed when a write fails
            }
            if (path == "/replay" && ctx.Request.HttpMethod == "POST")
            {
                // re-send a saved capture (captures/*.json) to Darker Sim — for testing without the game
                using var rd = new StreamReader(ctx.Request.InputStream, Encoding.UTF8);
                var body = (await rd.ReadToEndAsync()).ReplaceLineEndings(" ");
                var n = Broadcast("data: " + body + "\n\n");
                var sent = Encoding.UTF8.GetBytes($"sent to {n} Darker Sim tab(s)\n");
                res.ContentType = "text/plain"; res.OutputStream.Write(sent); res.Close(); return;
            }
            if (path == "/result" && ctx.Request.HttpMethod == "POST")
            {
                using var rd = new StreamReader(ctx.Request.InputStream, Encoding.UTF8);
                var body = await rd.ReadToEndAsync();
                try
                {
                    using var doc = JsonDocument.Parse(body);
                    Result?.Invoke(doc.RootElement.GetProperty("ok").GetBoolean(), doc.RootElement.GetProperty("text").GetString() ?? "");
                }
                catch { }
                res.StatusCode = 204; res.Close(); return;
            }
            var msg = Encoding.UTF8.GetBytes($"DarkerLink is running. Darker Sim tabs connected: {Clients}\n");
            res.ContentType = "text/plain";
            res.OutputStream.Write(msg);
            res.Close();
        }
        catch { try { res.Abort(); } catch { } }
    }

    static bool Write(HttpListenerResponse r, string s)
    {
        try
        {
            var b = Encoding.UTF8.GetBytes(s);
            lock (r) { r.OutputStream.Write(b); r.OutputStream.Flush(); }
            return true;
        }
        catch { return false; }
    }

    public int Broadcast(string frame)
    {
        int ok = 0;
        foreach (var (id, r) in clients)
        {
            if (Write(r, frame)) ok++;
            else { clients.TryRemove(id, out _); try { r.Abort(); } catch { } }
        }
        return ok;
    }

    public int Send(object payload) => Broadcast("data: " + JsonSerializer.Serialize(payload) + "\n\n");
}

/// <summary>Click-through, non-activating toast drawn over the game (never steals focus).</summary>
public sealed class Toast : Form
{
    string text = "";
    Color accent = Color.FromArgb(226, 184, 119);
    readonly System.Windows.Forms.Timer hide = new() { Interval = 3500 };

    public Toast()
    {
        FormBorderStyle = FormBorderStyle.None;
        ShowInTaskbar = false;
        TopMost = true;
        StartPosition = FormStartPosition.Manual;
        BackColor = Color.FromArgb(16, 13, 10);
        Opacity = 0.95;
        DoubleBuffered = true;
        Font = new Font("Segoe UI Semibold", 11f);
        hide.Tick += (_, _) => { hide.Stop(); Hide(); };
    }

    protected override bool ShowWithoutActivation => true;
    protected override CreateParams CreateParams
    {
        get
        {
            const int WS_EX_TOPMOST = 0x8, WS_EX_TOOLWINDOW = 0x80, WS_EX_NOACTIVATE = 0x08000000, WS_EX_TRANSPARENT = 0x20, WS_EX_LAYERED = 0x80000;
            var cp = base.CreateParams;
            cp.ExStyle |= WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED;
            return cp;
        }
    }

    public void Show(string message, bool ok, Point near, int ms = 3500)
    {
        text = message;
        accent = ok ? Color.FromArgb(226, 184, 119) : Color.FromArgb(224, 103, 90);
        var size = TextRenderer.MeasureText(text, Font, new Size(520, 0), TextFormatFlags.WordBreak);
        Size = new Size(size.Width + 34, size.Height + 22);
        var screen = Screen.FromPoint(near).WorkingArea;
        Location = new Point(Math.Clamp(near.X + 24, screen.Left, screen.Right - Width), Math.Clamp(near.Y + 24, screen.Top, screen.Bottom - Height));
        Invalidate();
        if (!Visible) base.Show();
        hide.Interval = ms;
        hide.Stop(); hide.Start();
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        using var border = new Pen(accent, 2);
        g.DrawRectangle(border, 1, 1, Width - 3, Height - 3);
        using var bar = new SolidBrush(accent);
        g.FillRectangle(bar, 1, 1, 5, Height - 2);
        TextRenderer.DrawText(g, text, Font, new Rectangle(18, 11, Width - 28, Height - 18), Color.FromArgb(235, 226, 205), TextFormatFlags.WordBreak);
    }
}

/// <summary>Hidden window that receives WM_HOTKEY.</summary>
public sealed class HotkeyWindow : NativeWindow, IDisposable
{
    [DllImport("user32.dll")] static extern bool RegisterHotKey(IntPtr hWnd, int id, uint mods, uint vk);
    [DllImport("user32.dll")] static extern bool UnregisterHotKey(IntPtr hWnd, int id);
    const int WM_HOTKEY = 0x0312;
    readonly Dictionary<int, Action> actions = new();

    public HotkeyWindow() => CreateHandle(new CreateParams());

    /// <summary>"Ctrl+Alt+A" → registered; returns an error text or null.</summary>
    public string? Register(string combo, Action action)
    {
        uint mods = 0x4000; // MOD_NOREPEAT
        Keys key = Keys.None;
        foreach (var part in combo.Split('+', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries))
        {
            switch (part.ToLowerInvariant())
            {
                case "ctrl" or "control": mods |= 0x2; break;
                case "shift": mods |= 0x4; break;
                case "alt": mods |= 0x1; break;
                case "win": mods |= 0x8; break;
                default:
                    if (!Enum.TryParse(part, true, out key)) return $"unknown key '{part}' in {combo}";
                    break;
            }
        }
        if (key == Keys.None) return $"no key in {combo}";
        int id = actions.Count + 1;
        if (!RegisterHotKey(Handle, id, mods, (uint)key)) return $"{combo} is already used by another program";
        actions[id] = action;
        return null;
    }

    protected override void WndProc(ref Message m)
    {
        if (m.Msg == WM_HOTKEY && actions.TryGetValue((int)m.WParam, out var a)) a();
        base.WndProc(ref m);
    }

    public void Dispose()
    {
        foreach (var id in actions.Keys) UnregisterHotKey(Handle, id);
        DestroyHandle();
    }
}

public sealed class App : ApplicationContext
{
    readonly Config cfg = Config.Load();
    readonly LinkServer server;
    readonly Toast toast = new();
    readonly HotkeyWindow hotkeys = new();
    readonly NotifyIcon tray;
    readonly SynchronizationContext ui;
    Point lastCursor;

    static string CaptureDir => Path.Combine(Config.Dir, "captures");

    public App()
    {
        ui = SynchronizationContext.Current ?? new WindowsFormsSynchronizationContext();
        server = new LinkServer(cfg.Port);
        server.Result += (ok, text) => ui.Post(_ => toast.Show(text, ok, lastCursor), null);
        try { server.Start(); }
        catch (Exception e) { MessageBox.Show($"Could not open localhost:{cfg.Port} ({e.Message}). Is DarkerLink already running?", "DarkerLink"); }

        var errors = new List<string?>
        {
            hotkeys.Register(cfg.HotkeyAttacker, () => _ = Capture("atk")),
            hotkeys.Register(cfg.HotkeyTarget, () => _ = Capture("tgt")),
        }.Where(e => e != null).ToList();

        var menu = new ContextMenuStrip();
        menu.Items.Add($"{cfg.HotkeyAttacker}  →  add hovered item to Attacker").Enabled = false;
        menu.Items.Add($"{cfg.HotkeyTarget}  →  add hovered item to Target").Enabled = false;
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Open Darker Sim", null, (_, _) => Open(cfg.SimUrl));
        menu.Items.Add("Open captures folder", null, (_, _) => { Directory.CreateDirectory(CaptureDir); Open(CaptureDir); });
        menu.Items.Add("Edit settings (restart after)", null, (_, _) => Open(Config.FilePath));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Exit", null, (_, _) => ExitThread());
        tray = new NotifyIcon { Icon = SystemIcons.Shield, Text = "DarkerLink", ContextMenuStrip = menu, Visible = true };
        tray.DoubleClick += (_, _) => Open(cfg.SimUrl);
        tray.ShowBalloonTip(4000, "DarkerLink running",
            errors.Count > 0 ? string.Join("\n", errors!) : $"Hover an item in game and press {cfg.HotkeyAttacker} (Attacker) or {cfg.HotkeyTarget} (Target).",
            errors.Count > 0 ? ToolTipIcon.Warning : ToolTipIcon.Info);
    }

    static void Open(string target) => Process.Start(new ProcessStartInfo(target) { UseShellExecute = true });

    sealed record Job(Bitmap Bmp, string Side, Rectangle Rect, Point Cursor);
    readonly System.Threading.Channels.Channel<Job> jobs = System.Threading.Channels.Channel.CreateBounded<Job>(
        new System.Threading.Channels.BoundedChannelOptions(20) { FullMode = System.Threading.Channels.BoundedChannelFullMode.DropOldest });
    int queued;
    bool workerStarted;

    /// <summary>Hotkey handler: only grabs the screen (a few ms) and queues it, so you can capture item after item.
    /// OCR + sending happen on a background worker, in capture order.</summary>
    Task Capture(string side)
    {
        lastCursor = Cursor.Position;
        try
        {
            if (server.Clients == 0)
            {
                toast.Show($"Darker Sim is not open — start it at {cfg.SimUrl}", false, lastCursor);
                return Task.CompletedTask;
            }
            var screen = Screen.FromPoint(lastCursor).Bounds;
            var rect = Rectangle.FromLTRB(
                Math.Max(screen.Left, lastCursor.X - cfg.CaptureLeft), Math.Max(screen.Top, lastCursor.Y - cfg.CaptureUp),
                Math.Min(screen.Right, lastCursor.X + cfg.CaptureRight), Math.Min(screen.Bottom, lastCursor.Y + cfg.CaptureDown));
            var bmp = new Bitmap(rect.Width, rect.Height, PixelFormat.Format32bppArgb);
            using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(rect.Location, Point.Empty, rect.Size);
            var n = Interlocked.Increment(ref queued);
            jobs.Writer.TryWrite(new Job(bmp, side, rect, lastCursor));
            if (!workerStarted) { workerStarted = true; _ = Task.Run(Worker); }
            toast.Show(n > 1 ? $"Captured ✓  ({n} reading…)" : "Captured ✓  reading…", true, lastCursor, 1500);
        }
        catch (Exception e)
        {
            toast.Show("DarkerLink error: " + e.Message, false, lastCursor);
        }
        return Task.CompletedTask;
    }

    async Task Worker()
    {
        await foreach (var job in jobs.Reader.ReadAllAsync())
        {
            try
            {
                var lines = await Ocr.Read(job.Bmp);
                var payload = new { type = "item", side = job.Side, width = job.Rect.Width, height = job.Rect.Height,
                    cursor = new { x = job.Cursor.X - job.Rect.Left, y = job.Cursor.Y - job.Rect.Top }, lines };
                lastCursor = job.Cursor; // the result toast shows where that item was
                if (server.Send(payload) == 0) ui.Post(_ => toast.Show("Darker Sim disconnected — reload the page", false, job.Cursor), null);
                // Darker Sim answers via /result → toast
                if (cfg.SaveCaptures) SaveCapture(job.Bmp, payload);
            }
            catch (Exception e)
            {
                ui.Post(_ => toast.Show("DarkerLink error: " + e.Message, false, job.Cursor), null);
            }
            finally
            {
                job.Bmp.Dispose();
                Interlocked.Decrement(ref queued);
            }
        }
    }

    static void SaveCapture(Bitmap bmp, object payload)
    {
        Directory.CreateDirectory(CaptureDir);
        var stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss-fff");
        bmp.Save(Path.Combine(CaptureDir, stamp + ".png"), ImageFormat.Png);
        File.WriteAllText(Path.Combine(CaptureDir, stamp + ".json"), JsonSerializer.Serialize(payload, new JsonSerializerOptions { WriteIndented = true }));
        foreach (var old in new DirectoryInfo(CaptureDir).GetFiles().OrderByDescending(f => f.Name).Skip(40)) old.Delete();
    }

    protected override void ExitThreadCore()
    {
        tray.Visible = false;
        hotkeys.Dispose();
        base.ExitThreadCore();
    }
}

public static class Program
{
    [STAThread]
    static int Main(string[] args)
    {
        // test mode: DarkerLink.exe --ocr image.png  → writes image.ocr.json (no UI)
        if (args.Length >= 2 && args[0] == "--ocr")
        {
            if (args.Length > 2) Ocr.ScaleOverride = double.Parse(args[2], System.Globalization.CultureInfo.InvariantCulture);
            using var bmp = new Bitmap(args[1]);
            using var argb = bmp.Clone(new Rectangle(0, 0, bmp.Width, bmp.Height), PixelFormat.Format32bppArgb);
            var lines = Ocr.Read(argb).GetAwaiter().GetResult();
            File.WriteAllText(Path.ChangeExtension(args[1], ".ocr.json"),
                JsonSerializer.Serialize(new { type = "item", side = "atk", width = bmp.Width, height = bmp.Height, lines }, new JsonSerializerOptions { WriteIndented = true }));
            return 0;
        }
        using var single = new Mutex(true, "DarkerLink.single", out bool first);
        if (!first) { MessageBox.Show("DarkerLink is already running (see the tray).", "DarkerLink"); return 0; }
        ApplicationConfiguration.Initialize();
        Application.Run(new App());
        return 0;
    }
}
