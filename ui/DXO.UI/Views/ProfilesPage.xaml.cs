using System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Storage.Pickers;

namespace DXO.UI.Views;

public sealed partial class ProfilesPage : Page
{
    public ProfilesPage() => InitializeComponent();

    private async void Load_Click(object sender, RoutedEventArgs e)
    {
        var picker = new FileOpenPicker();
        picker.FileTypeFilter.Add(".toml");
        // Unpackaged WinUI 3 apps must hand the picker a window handle.
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(((App)Application.Current).MainWindowRef!);
        WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);

        var file = await picker.PickSingleFileAsync();
        if (file is null) return;
        bool ok = App.Engine?.LoadProfile(file.Path) ?? false;
        Result.Text = ok ? $"Loaded {file.Name}" : $"Could not load {file.Name}";
    }
}
