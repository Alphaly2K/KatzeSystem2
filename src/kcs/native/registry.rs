#[derive(Clone, Copy)]
pub(super) enum CoreHandler {
    Text,
    Scalar,
    Settings,
    Storage,
    System,
    Data,
    MapData,
    Sequence,
    RecordStack,
    Vector,
    Continue,
    Zero,
    Unknown,
}

pub(super) fn canonical(index: u32) -> u32 {
    match index {
        675 | 885 | 894 => 515,
        676 | 886 | 895 => 516,
        888 => 897,
        889 => 898,
        890 => 899,
        893 => 902,
        _ => index,
    }
}

pub(super) fn core(index: u32) -> CoreHandler {
    match index {
        13..=26
        | 37
        | 38
        | 40..=48
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
        33 | 34 | 50..=64 | 318 | 319 | 397 => CoreHandler::Scalar,
        359..=361
        | 533
        | 565
        | 566
        | 741
        | 789
        | 790
        | 852
        | 938..=964
        | 999..=1001
        | 1036
        | 1050
        | 1053
        | 1062 => CoreHandler::Settings,
        243 | 253 | 419..=423 | 515..=518 | 869 | 870 | 872..=875 | 896..=899 | 902 | 1052 => {
            CoreHandler::Storage
        }
        238 | 242 | 244..=248 | 252 | 254 | 290 | 389 | 390 | 469 | 833..=836 => CoreHandler::Data,
        900 | 901 => CoreHandler::MapData,
        424 | 425 => CoreHandler::Sequence,
        863..=868 => CoreHandler::RecordStack,
        352..=358 => CoreHandler::Vector,
        1
        | 4
        | 5
        | 6
        | 8
        | 9
        | 35
        | 36
        | 65
        | 68
        | 71
        | 75
        | 76
        | 149
        | 207
        | 209..=213
        | 284
        | 392
        | 393
        | 465
        | 629
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
        | 399
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
