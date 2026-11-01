using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace DXO.UI.Views;

public sealed record PlaceholderInfo(string Title, string What, string When);

public sealed partial class PlaceholderPage : Page
{
    public PlaceholderPage() => InitializeComponent();

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        var info = e.Parameter as PlaceholderInfo ?? new PlaceholderInfo("Coming later", "", "");
        TitleText.Text = info.Title;
        WhatText.Text = info.What;
        WhenText.Text = string.IsNullOrEmpty(info.When) ? "Coming later" : $"Coming in {info.When}";
    }
}
