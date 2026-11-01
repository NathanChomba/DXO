using Microsoft.UI.Xaml.Controls;

namespace DXO.UI.Views;

public sealed partial class VirtualDevicePage : Page
{
    public VirtualDevicePage() => InitializeComponent();

    private void Mode_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        // TODO (M3): add dxo_set_virtual_target to ffi.rs and call it here.
        if (ModeNote is null) return; // fires once during InitializeComponent
        ModeNote.Text = Mode.SelectedIndex == 0
            ? "Native mode: no virtual controller. Games that only support Xbox controllers may not see your pad."
            : "Changing mode reconnects the virtual controller.";
    }
}
