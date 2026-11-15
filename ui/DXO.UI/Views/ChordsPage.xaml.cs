using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using DXO.UI.ViewModels;

namespace DXO.UI.Views;

/// Milestone 7: choose what each PS + button chord does, and what is in the
/// quick menu. Both tables live in the engine (chords.rs, radial.rs); this
/// page edits them as JSON and keeps a copy in settings.json so they come
/// back at the next start.
public sealed partial class ChordsPage : Page
{
    /// Buttons that can be chorded with PS: the engine's name, and the name printed on the pad.
    private static readonly (string Name, string Label)[] Buttons =
    {
        ("share", "Share"), ("options", "Options"), ("touchpad", "Touchpad"),
        ("l3", "L3 (left stick click)"), ("r3", "R3 (right stick click)"),
        ("up", "D-pad Up"), ("down", "D-pad Down"), ("left", "D-pad Left"), ("right", "D-pad Right"),
        ("l1", "L1"), ("r1", "R1"), ("l2", "L2"), ("r2", "R2"),
        ("cross", "Cross"), ("circle", "Circle"), ("square", "Square"), ("triangle", "Triangle"),
    };

    private const int MaxMenuItems = 12;

    private List<SysBindingModel> _chords = new();
    private List<RadialItemModel> _menu = new();
    private bool _loading;

    public ChordsPage()
    {
        InitializeComponent();
        OffInfo.IsOpen = !App.Settings.SystemChords;
        LoadChords();
        LoadMenu();
    }

    // ================= chords =================

    private void LoadChords()
    {
        var saved = SysBindingModel.ListFromJson(App.Engine?.ChordsJson ?? "");
        // One row per button, whether or not it has an action yet.
        _chords = Buttons.Select(b => new SysBindingModel
        {
            Button = b.Name,
            Action = saved.FirstOrDefault(s => s.Button.Equals(b.Name, StringComparison.OrdinalIgnoreCase))?.Action ?? new SysActionModel(),
        }).ToList();

        _loading = true;
        ChordRows.Children.Clear();
        for (int i = 0; i < _chords.Count; i++)
        {
            var row = new Grid { ColumnSpacing = 12 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(200) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.Children.Add(new TextBlock { Text = "PS + " + Buttons[i].Label, VerticalAlignment = VerticalAlignment.Center });
            var (box, param) = MakeEditor(_chords[i].Action, SaveChords);
            Grid.SetColumn(box, 1);
            Grid.SetColumn(param, 2);
            row.Children.Add(box);
            row.Children.Add(param);
            ChordRows.Children.Add(row);
        }
        _loading = false;
    }

    private void SaveChords()
    {
        string json = SysBindingModel.ListToJson(_chords);
        bool ok = App.Engine?.SetChordsJson(json) ?? false;
        if (ok)
        {
            App.Settings.ChordsJson = json;
            App.Settings.Save();
        }
        StatusText.Text = ok ? "Chords saved." : "The engine didn't accept that. Nothing was changed.";
    }

    private void ResetChords_Click(object sender, RoutedEventArgs e)
    {
        App.Engine?.SetChordsJson(""); // "" = DXO's own table
        App.Settings.ChordsJson = "";
        App.Settings.Save();
        LoadChords();
        StatusText.Text = "DXO's chords are back.";
    }

    // ================= quick menu =================

    private void LoadMenu()
    {
        _menu = RadialItemModel.ListFromJson(App.Engine?.RadialItemsJson ?? "");
        BuildMenuRows();
    }

    private void BuildMenuRows()
    {
        _loading = true;
        MenuRows.Children.Clear();
        foreach (var item in _menu)
        {
            var row = new Grid { ColumnSpacing = 12 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(200) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

            var label = new TextBox { Text = item.Label, PlaceholderText = "Name shown in the menu", MaxLength = 24 };
            var mine = item;
            label.LostFocus += (_, _) =>
            {
                if (_loading || mine.Label == label.Text.Trim()) return;
                mine.Label = label.Text.Trim();
                SaveMenu();
            };
            row.Children.Add(label);

            var (box, param) = MakeEditor(item.Action, SaveMenu, forMenu: true);
            Grid.SetColumn(box, 1);
            Grid.SetColumn(param, 2);
            row.Children.Add(box);
            row.Children.Add(param);

            var remove = new Button { Content = new FontIcon { Glyph = "", FontSize = 14 } };
            ToolTipService.SetToolTip(remove, "Remove this item");
            remove.Click += (_, _) =>
            {
                _menu.Remove(mine);
                BuildMenuRows();
                SaveMenu();
            };
            Grid.SetColumn(remove, 3);
            row.Children.Add(remove);
            MenuRows.Children.Add(row);
        }
        _loading = false;
    }

    private void SaveMenu()
    {
        string json = RadialItemModel.ListToJson(_menu);
        bool ok = App.Engine?.SetRadialItemsJson(json) ?? false;
        if (ok)
        {
            App.Settings.QuickMenuJson = json == "[]" ? "" : json; // nothing left = DXO's menu again
            App.Settings.Save();
        }
        StatusText.Text = !ok ? "The engine didn't accept that. Nothing was changed."
            : json == "[]" ? "The menu has no finished items, so DXO's own menu is in use."
            : "Quick menu saved. Items without a name or an action are left out.";
    }

    private void AddItem_Click(object sender, RoutedEventArgs e)
    {
        if (_menu.Count >= MaxMenuItems)
        {
            StatusText.Text = $"The menu holds {MaxMenuItems} items at most.";
            return;
        }
        _menu.Add(new RadialItemModel());
        BuildMenuRows();
        StatusText.Text = "Give the new item a name and an action.";
    }

    private void ResetMenu_Click(object sender, RoutedEventArgs e)
    {
        App.Engine?.SetRadialItemsJson("[]"); // an empty list = DXO's own menu
        App.Settings.QuickMenuJson = "";
        App.Settings.Save();
        LoadMenu();
        StatusText.Text = "DXO's menu is back.";
    }

    private void TryMenu_Click(object sender, RoutedEventArgs e)
    {
        bool ok = App.Engine?.SetRadialOpen(true) ?? false;
        StatusText.Text = ok ? "The menu is open: point with a stick, Circle closes it." : "Connect a controller first.";
    }

    // ================= shared: the action picker =================

    /// A drop-down of actions plus a text box for the two that need one
    /// ("Press keys", "Open an app"). Edits `action` in place and calls `changed`.
    private (ComboBox Box, TextBox Param) MakeEditor(SysActionModel action, Action changed, bool forMenu = false)
    {
        // The quick menu can't contain itself.
        var choices = SysActionModel.Choices.Where(c => !forMenu || c.Type != "radial_menu").ToArray();
        var box = new ComboBox { Width = 250, VerticalAlignment = VerticalAlignment.Center };
        foreach (var (_, label) in choices) box.Items.Add(label);
        box.SelectedIndex = Math.Max(0, Array.FindIndex(choices, c => c.Type == action.Type));

        var param = new TextBox { VerticalAlignment = VerticalAlignment.Center };
        void ShowParam()
        {
            param.Visibility = action.Type is "keys" or "launch" ? Visibility.Visible : Visibility.Collapsed;
            param.PlaceholderText = action.Type == "keys" ? "For example Ctrl+Shift+Esc" : "A program, file, folder or web address";
            param.Text = action.Type == "keys" ? KeyNames.Format(action.Keys) : action.Type == "launch" ? action.Target ?? "" : "";
        }
        ShowParam();

        box.SelectionChanged += (_, _) =>
        {
            if (_loading) return;
            action.Type = choices[Math.Max(0, box.SelectedIndex)].Type;
            action.Normalize();
            bool was = _loading;
            _loading = true;
            ShowParam();
            _loading = was;
            changed();
        };
        param.LostFocus += (_, _) =>
        {
            if (_loading) return;
            if (action.Type == "keys")
            {
                action.Keys = KeyNames.Parse(param.Text);
                param.Text = KeyNames.Format(action.Keys); // show what was understood
            }
            else if (action.Type == "launch") action.Target = param.Text.Trim();
            else return;
            changed();
        };
        return (box, param);
    }
}
