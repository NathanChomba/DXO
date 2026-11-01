using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace DXO.UI.Views;

public sealed partial class HomePage : Page
{
    public HomePage() => InitializeComponent();

    private void Tile_Click(object sender, RoutedEventArgs e)
    {
        var tag = (string)((FrameworkElement)sender).Tag;
        if (tag == "settings")
            ((MainWindow?)((App)Application.Current).MainWindowRef)?.OpenSettings();
        else
            MainWindow.NavigateTo(tag);
    }
}
