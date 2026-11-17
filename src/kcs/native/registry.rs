#[derive(Clone, Copy)]
pub(super) enum CoreHandler {
    Text,
    Memory,
    Rectangle,
    Scalar,
    Settings,
    Storage,
    File,
    Database,
    System,
    Window,
    Achievements,
    Matrix,
    Data,
    MapData,
    Sequence,
    RecordStack,
    Vector,
    Continue,
    Zero,
    Unknown,
}

pub(crate) fn canonical(index: u32) -> u32 {
    match index {
        705 => 122,
        675 | 885 | 894 => 515,
        676 | 886 | 895 => 516,
        887 => 896,
        888 => 897,
        889 => 898,
        890 => 899,
        891 => 900,
        892 => 901,
        893 => 902,
        _ => index,
    }
}

pub(super) fn core(index: u32) -> CoreHandler {
    match index {
        441 | 628 | 845..=847 => CoreHandler::Window,
        1042 | 1043 => CoreHandler::Achievements,
        29..=32 => CoreHandler::File,
        27 | 28 | 35 | 36 => CoreHandler::Memory,
        214 | 215 | 217 | 218 => CoreHandler::Rectangle,
        13..=26
        | 37..=49
        | 276..=278
        | 519
        | 539
        | 540
        | 598
        | 1002
        | 1045
        | 1046
        | 1051
        | 1063 => CoreHandler::Text,
        904 => CoreHandler::Text,
        33 | 34 | 50..=64 | 318..=320 | 397 | 461 | 786 | 874 | 875 => CoreHandler::Scalar,
        359..=361
        | 533
        | 565
        | 566
        | 605
        | 606
        | 607
        | 674
        | 741
        | 789
        | 790
        | 844
        | 852
        | 938..=964
        | 995
        | 999..=1001
        | 1036
        | 1050
        | 1053
        | 1062 => CoreHandler::Settings,
        243 | 253 | 419..=423 | 459 | 515..=518 | 896..=899 | 902 | 1052 => CoreHandler::Storage,
        869..=873 | 878 => CoreHandler::Database,
        238 | 242 | 244..=248 | 252 | 254 | 290 | 389 | 390 | 469 | 833..=836 | 990 | 1031 => {
            CoreHandler::Data
        }
        900 | 901 => CoreHandler::MapData,
        424 | 425 => CoreHandler::Sequence,
        863..=868 => CoreHandler::RecordStack,
        350..=358 => CoreHandler::Vector,
        204..=207 => CoreHandler::Matrix,
        1
        | 4
        | 5
        | 6
        | 8
        | 9
        | 10
        | 11
        | 65
        | 68
        | 71
        | 75
        | 76
        | 149
        | 209..=213
        | 567
        | 787
        | 392
        | 393
        | 445
        | 465
        | 609
        | 610
        | 629
        | 1039
        | 1040
        | 1049 => CoreHandler::System,
        0
        | 2
        | 3
        | 141
        | 169
        | 170
        | 172
        | 173
        | 180
        | 182
        | 184
        | 185
        | 187..=191
        | 429
        | 439
        | 543..=548
        | 550
        | 552
        | 553
        | 556
        | 559..=563
        | 573
        | 574
        | 578
        | 579
        | 581
        | 626
        | 627
        | 642..=644
        | 652
        | 656
        | 660
        | 661
        | 670..=673
        | 686
        | 687
        | 906..=908
        | 910..=924
        | 926..=933
        | 965
        | 966
        | 974..=976
        | 1006
        | 1025
        | 1047
        | 1048 => CoreHandler::Continue,
        171
        | 174..=179
        | 181
        | 183
        | 186
        | 236
        | 432
        | 551
        | 554
        | 555
        | 575
        | 580
        | 909
        | 925
        | 1027..=1030 => CoreHandler::Zero,
        _ => CoreHandler::Unknown,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RuntimeHandler {
    FileOpen,
    Property,
    Configuration,
    DebugWindow,
    Export,
    SystemTask,
    ImageExport,
    Emocon,
    Backlog,
    LoadProgram,
    LoadParticle,
    Movie,
    Transition,
    NameTable,
    SectionTable,
    Cst,
    Motion,
    SequenceFile,
    DatabaseLoad,
    SharedRecord,
    FlagBackup,
    SoundLoop,
    SoundControl,
    SoundLoad,
    Input,
    Message,
    TextWindow,
    Fes,
    PointerPosition,
    Graphics,
    Task,
    MediaRecord,
    StateRecord,
    Resource,
    ImageResource,
    Core,
}

pub(crate) fn runtime(index: u32) -> RuntimeHandler {
    match canonical(index) {
        1009 => RuntimeHandler::ImageExport,
        600 | 601 | 1018..=1023 => RuntimeHandler::SystemTask,
        123..=125 | 418 | 557 => RuntimeHandler::TextWindow,
        29 => RuntimeHandler::FileOpen,
        453 => RuntimeHandler::Property,
        361 => RuntimeHandler::Configuration,
        279..=283 | 287 | 288 => RuntimeHandler::DebugWindow,
        690..=692 => RuntimeHandler::Export,
        1055..=1061 => RuntimeHandler::Emocon,
        696..=700 | 785 | 850 | 851 | 876 | 977 | 1034 => RuntimeHandler::Backlog,
        143 => RuntimeHandler::LoadProgram,
        271 | 693 => RuntimeHandler::LoadParticle,
        265..=270 | 417 | 788 | 997 | 998 => RuntimeHandler::Movie,
        196..=201
        | 203
        | 520..=524
        | 527
        | 528
        | 530..=532
        | 853..=857
        | 860
        | 861
        | 905
        | 967..=973 => RuntimeHandler::Transition,
        633..=640 => RuntimeHandler::NameTable,
        683..=685 => RuntimeHandler::SectionTable,
        323..=338
        | 608
        | 385
        | 386
        | 934
        | 978
        | 979
        | 980
        | 981
        | 982
        | 991..=994
        | 1033
        | 1037
        | 1038
        | 1064 => RuntimeHandler::Cst,
        294..=317 | 388 | 440 | 514 | 535 | 586 => RuntimeHandler::Motion,
        458 => RuntimeHandler::SequenceFile,
        869 => RuntimeHandler::DatabaseLoad,
        240 | 250 => RuntimeHandler::SharedRecord,
        609 | 610 => RuntimeHandler::FlagBackup,
        400 => RuntimeHandler::SoundLoop,
        156..=168 | 347..=349 | 903 => RuntimeHandler::SoundControl,
        604 | 935 | 936 => RuntimeHandler::SoundLoad,
        145..=148 | 150 | 154 | 285 | 286 | 401 | 403 | 404 | 937 => RuntimeHandler::Input,
        611..=621 | 695 | 884 => RuntimeHandler::Message,
        447..=451 | 630 | 632 | 645..=647 | 649 | 650 | 665..=668 | 882 | 883 => {
            RuntimeHandler::Fes
        }
        149 => RuntimeHandler::PointerPosition,
        113
        | 78
        | 79
        | 80
        | 81
        | 82
        | 83
        | 84..=107
        | 256
        | 257
        | 258
        | 260
        | 261
        | 262
        | 263
        | 339..=342
        | 343..=345
        | 346
        | 394
        | 398
        | 399
        | 402
        | 433
        | 455
        | 731..=732
        | 462..=464
        | 466
        | 534
        | 541
        | 542
        | 568
        | 570
        | 571
        | 572
        | 625
        | 653
        | 654
        | 655
        | 657
        | 663
        | 701
        | 702
        | 739
        | 740
        | 848
        | 849
        | 858
        | 859
        | 862
        | 879
        | 880
        | 881
        | 988
        | 989
        | 1065..=1069 => RuntimeHandler::Graphics,
        66
        | 67
        | 69
        | 70
        | 72
        | 73
        | 75
        | 76
        | 77
        | 108..=110
        | 272..=275
        | 426
        | 427
        | 452
        | 454
        | 577
        | 651
        | 694 => RuntimeHandler::Task,
        241 | 251 | 387 => RuntimeHandler::MediaRecord,
        576 | 688 | 689 => RuntimeHandler::StateRecord,
        111
        | 112
        | 216
        | 114..=120
        | 121
        | 122
        | 284
        | 362..=374
        | 376..=378
        | 434..=438
        | 468
        | 536..=538
        | 558
        | 564
        | 590..=597
        | 641
        | 1026 => RuntimeHandler::Resource,
        681 | 682 | 710..=730 | 733..=737 | 877 => RuntimeHandler::ImageResource,
        _ => RuntimeHandler::Core,
    }
}
