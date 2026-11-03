using Microsoft.UI.Xaml.Controls;

namespace DXO.UI.Views;

public sealed partial class AboutPage : Page
{
    public AboutPage()
    {
        InitializeComponent();
        VersionText.Text = $"DXO v{App.Version}";
    }
}
